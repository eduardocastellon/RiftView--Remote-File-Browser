# Third-party notices: planning notes

Running list of third-party code and assets that need a license notice.
Compile these later into `THIRD_PARTY_NOTICES.md` (repo) and an
"Open-source licenses" row under Settings → About (in-app).

General rule for permissive licenses (MIT, ISC, BSD, Apache-2.0): commercial
use is allowed; ship the copyright + license text with the app. Copy license
text **verbatim** from each project's LICENSE file, don't paraphrase.

## Logged so far

### Lucide icons
- **What:** SVG path data copied into `src/components/icons.tsx`
  (chevron-left, chevron-right, search, menu, x, house, log-out).
- **License:** ISC. Some icons (incl. chevrons, search, menu) originate from
  **Feather** (MIT, Cole Bemis); Lucide's single LICENSE file covers both.
- **Source:** https://lucide.dev · LICENSE: https://github.com/lucide-icons/lucide/blob/main/LICENSE
- **Status:** `icons.tsx` has a short credit comment only, not the full notice.
- **When adding icons:** append the icon name to the list above.

### react-native-svg
- **What:** npm dependency (`react-native-svg` 15.16.0); renders the icons natively.
- **License:** MIT.
- **Source:** https://github.com/software-mansion/react-native-svg (LICENSE in repo / `node_modules/react-native-svg/LICENSE`).

## Still to compile
- [ ] Remaining npm dependencies (React Native, Expo modules, @noble/*, etc.).
      Most are MIT; a license scanner can generate the list from `package.json` /
      `node_modules`, e.g. `npx license-checker --production --summary`.
- [ ] Android native libraries (e.g. OkHttp in `modules/riftview-tls`, Apache-2.0).
- [ ] Any other icons / images in `assets/`. Note their source and license.
- [ ] Write `THIRD_PARTY_NOTICES.md`.
- [ ] Add the "Open-source licenses" screen under Settings → About.

## Avoid
- Assets whose license forbids commercial use, paid icon packs without a
  license, or other companies' logos/trademarks.
