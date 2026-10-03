#!/usr/bin/env bash
# Sign an unsigned Play bundle with the upload key, not the app-signing key.
set -euo pipefail

if [[ $# != 2 ]]; then
  echo "Usage: $0 unsigned.aab signed.aab" >&2
  exit 2
fi
: "${BABYTRACK_UPLOAD_KEYSTORE:?Set the upload keystore path}"
: "${BABYTRACK_UPLOAD_STORE_PASSWORD:?Set the keystore password}"
: "${BABYTRACK_UPLOAD_KEY_PASSWORD:?Set the key password}"
export BABYTRACK_UPLOAD_STORE_PASSWORD BABYTRACK_UPLOAD_KEY_PASSWORD
upload_alias="${BABYTRACK_UPLOAD_ALIAS:-upload}"
unsigned_bundle="$1"
signed_bundle="$2"
if [[ -e "$signed_bundle" || -e "$signed_bundle.pem" || -e "$signed_bundle.sha256" ]]; then
  echo "Refusing to overwrite an existing signing output" >&2
  exit 1
fi

# Refuse dependency-only ABIs, absent core libraries, and pre-signed inputs.
python3 - "$unsigned_bundle" <<'PY'
import sys
from zipfile import ZipFile

with ZipFile(sys.argv[1]) as bundle:
    names = bundle.namelist()
    if len(names) != len(set(names)):
        raise SystemExit('Duplicate ZIP entries in bundle')
    abis = {name.split('/')[2] for name in names if name.startswith('base/lib/')}
    if abis != {'arm64-v8a', 'x86_64'}:
        raise SystemExit(f'Unexpected bundle ABIs: {sorted(abis)}')
    for abi in abis:
        if f'base/lib/{abi}/libbabytrack_core_ffi.so' not in names:
            raise SystemExit(f'Missing Rust core for {abi}')
    if any(name.upper().startswith('META-INF/') and
           name.upper().endswith(('.RSA', '.DSA', '.EC', '.SF')) for name in names):
        raise SystemExit('Input bundle must be unsigned')
PY

mkdir -p "$(dirname "$signed_bundle")"
signing_tmp="$(mktemp -d "$(dirname "$signed_bundle")/.bundle-sign.XXXXXX")"
trap 'rm -rf "$signing_tmp"' EXIT
jarsigner -keystore "$BABYTRACK_UPLOAD_KEYSTORE" \
  -storepass:env BABYTRACK_UPLOAD_STORE_PASSWORD \
  -keypass:env BABYTRACK_UPLOAD_KEY_PASSWORD \
  -sigalg SHA256withRSA -digestalg SHA-256 \
  -signedjar "$signing_tmp/signed.aab" "$unsigned_bundle" "$upload_alias"
# Trust this exact upload keystore/alias; fail on unsigned entries or bad signatures.
jarsigner -verify -strict -keystore "$BABYTRACK_UPLOAD_KEYSTORE" \
  -storepass:env BABYTRACK_UPLOAD_STORE_PASSWORD \
  "$signing_tmp/signed.aab" "$upload_alias"
keytool -exportcert -rfc -keystore "$BABYTRACK_UPLOAD_KEYSTORE" \
  -storepass:env BABYTRACK_UPLOAD_STORE_PASSWORD -alias "$upload_alias" \
  -file "$signing_tmp/certificate.pem"
python3 - "$signing_tmp/signed.aab" "$signed_bundle" "$signing_tmp/checksum" <<'PY'
import hashlib
import sys
from pathlib import Path

digest = hashlib.sha256(Path(sys.argv[1]).read_bytes()).hexdigest()
Path(sys.argv[3]).write_text(f'{digest}  {Path(sys.argv[2]).name}\n')
PY
mv "$signing_tmp/signed.aab" "$signed_bundle"
mv "$signing_tmp/certificate.pem" "$signed_bundle.pem"
mv "$signing_tmp/checksum" "$signed_bundle.sha256"
echo "Verified upload-signed bundle: $signed_bundle"
