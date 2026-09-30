//! History helpers shared inside the relay store.

use super::*;
pub(super) fn lower_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 15) as usize] as char);
    }
    out
}

pub(super) fn load_page_entries(
    db: &Connection,
    family: [u8; 16],
    after: u64,
    filter_kind: Option<u8>,
) -> Result<(Vec<receipt::RelayEntry>, bool), Error> {
    const MAX_ENTRY_BYTES: usize = 4 * 1024 * 1024 - 128;
    let mut query = db.prepare(
        "SELECT cursor,kind,committed_bytes FROM entries
         WHERE family_id=?1 AND cursor>?2 AND (?3=0 OR kind=?3)
         ORDER BY cursor LIMIT 257",
    )?;
    let mut rows = query.query(params![
        &family[..],
        i64::try_from(after).map_err(|_| Error::Invalid("cursor range"))?,
        i64::from(filter_kind.unwrap_or(0)),
    ])?;
    let mut entries = Vec::new();
    let mut used = 0usize;
    let mut has_more = false;
    while let Some(row) = rows.next()? {
        if entries.len() == 256 {
            has_more = true;
            break;
        }
        let cursor: i64 = row.get(0)?;
        let kind: i64 = row.get(1)?;
        let entry = receipt::RelayEntry {
            cursor: cursor
                .try_into()
                .map_err(|_| Error::Invalid("cursor range"))?,
            kind: kind
                .try_into()
                .map_err(|_| Error::Invalid("entry kind range"))?,
            committed_bytes: row.get(2)?,
        };
        let next = used
            .checked_add(receipt::page_entry_wire_len(&entry)?)
            .ok_or(Error::Invalid("page byte count overflow"))?;
        if next > MAX_ENTRY_BYTES {
            if entries.is_empty() {
                return Err(Error::Invalid("single entry exceeds page limit"));
            }
            has_more = true;
            break;
        }
        used = next;
        entries.push(entry);
    }
    Ok((entries, has_more))
}

// Join transitions have a fixed order, but data batches occupy the same global
// cursor space. Locate a control by its ordinal rather than assuming its cursor.
pub(super) fn verify_stored_genesis(
    db: &Connection,
    family: [u8; 16],
    stored_candidate: &[u8],
    relay_public: [u8; 32],
) -> Result<authority::GenesisCandidate, Error> {
    let (committed, reservation): (Vec<u8>, Vec<u8>) = db.query_row(
        "SELECT committed_bytes,reservation_hash FROM families WHERE family_id=?1 AND active=1",
        params![&family[..]],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if reservation != crypto::hash("genesis-reservation", stored_candidate)? {
        return Err(Error::Invalid("genesis reservation differs from candidate"));
    }
    let entry: Vec<u8> = db.query_row(
        "SELECT committed_bytes FROM entries WHERE family_id=?1 AND cursor=1 AND kind=1",
        params![&family[..]],
        |row| row.get(0),
    )?;
    if committed != entry {
        return Err(Error::Invalid(
            "genesis family row differs from committed log",
        ));
    }
    let receipt = receipt::verify_control_receipt(&entry, &relay_public)?;
    if receipt.family_id != family
        || receipt.cursor != 1
        || receipt.candidate_bytes != stored_candidate
    {
        return Err(Error::Invalid(
            "genesis candidate or receipt differs from committed log",
        ));
    }
    let genesis = authority::verify_genesis_candidate(&receipt.candidate_bytes, &relay_public)?;
    if receipt.relay_id != genesis.relay_id {
        return Err(Error::Invalid(
            "genesis relay differs from signed candidate",
        ));
    }
    Ok(genesis)
}

pub(super) fn control_at(
    db: &Connection,
    family: [u8; 16],
    ordinal: i64,
) -> Result<Vec<u8>, Error> {
    Ok(db.query_row(
        "SELECT committed_bytes FROM entries WHERE family_id=?1 AND kind=1 ORDER BY cursor LIMIT 1 OFFSET ?2",
        params![&family[..], ordinal],
        |r| r.get(0),
    )?)
}

pub(super) fn control_count(db: &Connection, family: [u8; 16]) -> Result<i64, Error> {
    Ok(db.query_row(
        "SELECT COUNT(*) FROM entries WHERE family_id=?1 AND kind=1",
        params![&family[..]],
        |r| r.get(0),
    )?)
}

pub(super) struct JoinPrefix {
    pub(super) genesis: authority::GenesisCandidate,
    pub(super) issue: authority::IssueCandidate,
    pub(super) claim: authority::ClaimCandidate,
    pub(super) claim_head: [u8; 32],
    pub(super) claim_time: i64,
    pub(super) cursor: i64,
    pub(super) controls: i64,
    pub(super) head: [u8; 32],
}
pub(super) fn load_join_prefix(
    db: &Connection,
    relay_public: [u8; 32],
    family: [u8; 16],
) -> Result<JoinPrefix, Error> {
    let (genesis_bytes,genesis_committed,cursor,head):(Vec<u8>,Vec<u8>,i64,Vec<u8>)=db.query_row(
        "SELECT candidate_bytes,committed_bytes,cursor,head_hash FROM families WHERE family_id=?1 AND active=1",
        params![&family[..]],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)),
    )?;
    let controls = control_count(db, family)?;
    if controls < 3 {
        return Err(Error::Invalid("claim not committed"));
    }
    let genesis = verify_stored_genesis(db, family, &genesis_bytes, relay_public)?;
    let genesis_head = crypto::hash("control-head", &genesis_committed)?;
    let issue_committed = control_at(db, family, 1)?;
    let issue = authority::verify_first_invite_issue(
        &control_candidate(&issue_committed)?,
        &genesis,
        genesis_head,
    )?;
    let issue_head = crypto::hash("control-head", &issue_committed)?;
    let claim_committed = control_at(db, family, 2)?;
    let claim = authority::verify_first_claim(
        &control_candidate(&claim_committed)?,
        &genesis,
        &issue,
        issue_head,
    )?;
    let claim_head = crypto::hash("control-head", &claim_committed)?;
    Ok(JoinPrefix {
        genesis,
        issue,
        claim,
        claim_head,
        claim_time: control_commit_time(&claim_committed)?,
        cursor,
        controls,
        head: head
            .try_into()
            .map_err(|_| Error::Invalid("Family head length"))?,
    })
}

pub(super) struct ProvedPrefix {
    pub(super) join: JoinPrefix,
    pub(super) challenge: authority::ChallengeCandidate,
    pub(super) proof: authority::ProofCandidate,
    pub(super) proof_head: [u8; 32],
    pub(super) proof_time: i64,
}
pub(super) struct AdmittedPrefix {
    pub(super) proved: ProvedPrefix,
    pub(super) admission: authority::AdmissionCandidate,
    pub(super) admission_head: [u8; 32],
    pub(super) admission_time: i64,
}
pub(super) fn load_admitted_prefix(
    db: &Connection,
    relay_public: [u8; 32],
    family: [u8; 16],
) -> Result<AdmittedPrefix, Error> {
    let proved = load_proved_prefix(db, relay_public, family)?;
    if proved.join.controls < 6 {
        return Err(Error::Invalid("first admission not committed"));
    }
    let committed = control_at(db, family, 5)?;
    let admission = authority::verify_first_admission(
        &control_candidate(&committed)?,
        &proved.join.genesis,
        &proved.join.issue,
        &proved.join.claim,
        &proved.challenge,
        &proved.proof,
        proved.proof_head,
    )?;
    Ok(AdmittedPrefix {
        proved,
        admission,
        admission_head: crypto::hash("control-head", &committed)?,
        admission_time: control_commit_time(&committed)?,
    })
}
pub(super) fn load_proved_prefix(
    db: &Connection,
    relay_public: [u8; 32],
    family: [u8; 16],
) -> Result<ProvedPrefix, Error> {
    let join = load_join_prefix(db, relay_public, family)?;
    if join.controls < 5 {
        return Err(Error::Invalid("proof not committed"));
    }
    let challenge_committed = control_at(db, family, 3)?;
    let challenge = authority::verify_first_challenge(
        &control_candidate(&challenge_committed)?,
        &join.genesis,
        &join.issue,
        &join.claim,
        join.claim_head,
    )?;
    let challenge_head = crypto::hash("control-head", &challenge_committed)?;
    let proof_committed = control_at(db, family, 4)?;
    let proof = authority::verify_first_proof(
        &control_candidate(&proof_committed)?,
        &join.genesis,
        &join.issue,
        &join.claim,
        &challenge,
        challenge_head,
    )?;
    let proof_head = crypto::hash("control-head", &proof_committed)?;
    Ok(ProvedPrefix {
        join,
        challenge,
        proof,
        proof_head,
        proof_time: control_commit_time(&proof_committed)?,
    })
}
pub(super) fn control_commit_time(committed: &[u8]) -> Result<i64, Error> {
    let value = cbor::decode(committed)?;
    let Value::Map(fields) = value else {
        return Err(Error::Invalid("committed control not map"));
    };
    let Value::Array(receipt) = &fields[2].1 else {
        return Err(Error::Invalid("control receipt not array"));
    };
    if receipt.len() != 6 {
        return Err(Error::Invalid("control receipt length"));
    }
    let Value::Integer(time) = receipt[4] else {
        return Err(Error::Invalid("control time not integer"));
    };
    time.try_into()
        .map_err(|_| Error::Invalid("control time range"))
}

pub(super) fn fixed<const N: usize>(value: &Value) -> Result<[u8; N], Error> {
    let Value::Bytes(bytes) = value else {
        return Err(Error::Invalid("expected bytes"));
    };
    bytes
        .as_slice()
        .try_into()
        .map_err(|_| Error::Invalid("bytes length"))
}
pub(super) fn number(value: &Value) -> Result<u64, Error> {
    let Value::Integer(number) = value else {
        return Err(Error::Invalid("expected integer"));
    };
    (*number)
        .try_into()
        .map_err(|_| Error::Invalid("unsigned integer"))
}
