// English launch copy lives in one catalog so routes can be translated later.
export const copy = {
  app: 'Babytrack', preview: 'Early web preview', today: 'Today', history: 'History',
  family: 'Family', createFamily: 'Create a Family', noAccount: 'Start tracking here without an account.',
  addChild: 'Add a child', childName: 'Child name', birthDate: 'Birthday',
  growthSex: 'Sex for growth charts', unspecified: 'Not specified', female: 'Female', male: 'Male', other: 'Other',
  save: 'Save', cancel: 'Cancel', localOnly: 'On this browser',
  localDescription: 'This Family is saved in this browser profile. Create a shared Family on Android, then invite this browser to join it.',
  chooseChild: 'Choose a child', recent: 'Recent entries', emptyHistory: 'No entries yet.',
  addActivity: 'Add activity', diaper: 'Diaper', bottle: 'Bottle', note: 'Note',
  wet: 'Wet', dirty: 'Dirty', both: 'Both', dry: 'Dry',
  amount: 'Amount (mL)', content: 'Contents', formula: 'Formula', milk: 'Breast milk', mixed: 'Mixed', otherMilk: 'Other',
  noteText: 'Note', allEntries: 'All entries', todaySummary: 'Today at a glance',
  feeds: 'Feeds', diapers: 'Diapers', children: 'Children',
  localNotice: 'Local preview · this browser only', sharedNotice: 'Encrypted Family · synced through relay', birthUnknown: 'Age unknown',
  loading: 'Loading…', welcomeTitle: 'A calmer place to keep the little details.',
  welcomeDetail: 'A new Family stays in this browser profile. Use an invitation below to join an existing shared Family.',
  addChildPrompt: 'Add a child to start tracking.', type: 'Type',
  switchFamily: 'Switch Family',
  back: 'Back', breastFeed: 'Breastfeed', leftBreast: 'Left breast', rightBreast: 'Right breast',
  feedingNow: 'Feeding now', feedingPaused: 'Paused', tapSideToStart: 'Tap a side to start.',
  tapToStart: 'Start or resume', tapToPause: 'Tap to pause',
  breastTimerHint: 'Tap the active side to pause, or the other side to switch. Paused time is excluded.',
  completedSides: 'Completed sides', saveBreast: 'Save feeding', discardSession: 'Discard session',
  discardBreastConfirm: 'Discard this unsaved feeding session?',
  timerTooShort: 'Keep a side running for at least one second before pausing or saving.',
  timerTooLong: 'Feeding time cannot exceed four hours.',
  timerTooManySegments: 'A feeding can have up to eight sides. Save this session to finish.',
  timerEmpty: 'Start a side before saving.',
  invalidDuration: 'Use minutes:seconds, such as 5:30.',
  futureEnd: 'The edited feeding cannot end in the future.',
  edit: 'Edit', editBreast: 'Edit breastfeeding',
  editBreastHint: 'Change each side and its active time. Pause before a side is excluded from feeding totals. The original start time stays fixed.',
  side: 'Side', pauseBefore: 'Pause before (minutes:seconds)',
  durationMmSs: 'Feeding time (minutes:seconds)', removeSide: 'Remove side', addSide: 'Add side',
  saveChanges: 'Save changes',
  joinFamily: 'Join a Family', invitationLink: 'Invitation link', join: 'Join',
  invitationHint: 'Open or paste a link from a Family manager. This browser profile becomes a separate device.',
  waitingInvite: 'Checking the invitation', waitingChallenge: 'Claim sent · waiting for the manager device',
  waitingGrant: 'Proof sent · waiting for the Family key grant',
  joining: 'Checking encrypted Family access…', joinPending: 'Joining a Family',
  shared: 'Shared Family', sharedDescription: 'This browser has its own device grant. Entries are encrypted before relay upload.',
  syncNow: 'Sync now', syncReady: 'Up to date', syncMore: 'Loading more Family history', syncFailed: 'Saved here · waiting for a connection',
  savedPending: 'Saved here · waiting to sync',
  pendingJoinResume: 'Resume joining',
  sideNumber: (number) => `Side ${new Intl.NumberFormat().format(number)}`,
  breastEntry: (left, right) => `Breastfeed · left ${left} · right ${right}`,
  familyNumber: (number) => `Family ${new Intl.NumberFormat().format(number)}`,
  ageUnit: {
    day: { one: 'day', other: 'days' }, week: { one: 'week', other: 'weeks' },
    month: { one: 'month', other: 'months' }, year: { one: 'year', other: 'years' },
  },
};

export function ageLabel(birthDay) {
  if (birthDay == null) return copy.birthUnknown;
  const today = new Date();
  const birth = new Date(Number(birthDay) * 86400000);
  const days = Math.max(0, Math.floor((Date.UTC(today.getFullYear(), today.getMonth(), today.getDate()) - birth.getTime()) / 86400000));
  let months = (today.getFullYear() - birth.getUTCFullYear()) * 12 + today.getMonth() - birth.getUTCMonth();
  if (today.getDate() < birth.getUTCDate()) months -= 1;
  months = Math.max(0, months);
  const unit = days < 14 ? 'day' : months < 1 ? 'week' : months < 24 ? 'month' : 'year';
  const count = unit === 'day' ? days : unit === 'week' ? Math.floor(days / 7) :
    unit === 'month' ? months : Math.floor(months / 12);
  const form = new Intl.PluralRules().select(count);
  return `${new Intl.NumberFormat().format(count)} ${copy.ageUnit[unit][form] ?? copy.ageUnit[unit].other}`;
}
