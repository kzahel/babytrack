# 014: Lantern restyle

Status: active, opened 2026-10-10. The owner named the product Lantern and
approved a flat, quiet visual direction for the welcome and home screens
([mockups](../design/lantern/)). This workstream restyles the web client
across the board to match, built on modular design tokens, and then ports
the result to Android. The [interface design topic](../topics/interface-design-and-localization.md)
owns the tokens and screen composition; [013](013-web-android-parity.md)
delivered the web features being restyled.

## Goal and exclusions

Every web screen uses the Lantern tokens and components, the welcome and
Today screens match the approved mockups, and the visible product name is
Lantern. Android then adopts the same tokens and screens. Behavior, data,
and the core stay unchanged except where a screen needs new presentation
logic, which lives in tested presentation modules.

Excluded: new features, iOS, final icon artwork (line icons are drawn in
place), store listing changes, renaming internal identifiers (`babytrack`,
`org.babytrack.app`), and hosted deployment.

## Ordered delivery slices

1. [x] Record the approved mockups, the token table, and this plan.
2. [x] Web foundation: `styles/tokens.css` for light and dark, modular base,
   component, and screen styles replacing the single stylesheet, an icon
   component, and the Lantern name in web copy.
3. [x] Web shell and welcome: header with child switcher, age, date, and
   sync status; bottom and side navigation; the approved welcome and first
   child step.
4. [ ] Web Today: the now card opening the running timer's detail, since-last
   surfaces, six icon quick actions, the day ribbon with core totals, and
   recent entries. Ribbon layout is a tested presentation function.
5. [ ] Web remaining screens: capture forms, timers, History, entry editors,
   Family, child profile, backups, and sharing in the same components.
6. [ ] Android port: theme tokens and the welcome, Today, and shared
   components, then the remaining screens, with fixture-gallery renders.
7. [ ] Reconcile: full validation, gallery and browser screenshots against
   the mockups, topics, and the tactical index.

## Gates and completion

Each slice is its own validated commit with its browser or Android tests
updated. Light, dark, large text, and narrow and wide layouts are checked
with screenshots. Complete when web and Android match the approved
direction and all validation passes.
