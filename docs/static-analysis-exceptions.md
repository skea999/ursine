# Static analysis exceptions

Active exceptions (owner + expiry required; max 90 days):

| Item | Reason | Owner | Expiry |
|---|---|---|---|
| `apply`, `cosmic-config`, `cosmic-config-derive`, `cosmic-settings-daemon`, `cosmic-theme`, `dnd`, `iced_accessibility`, `mime 0.1.0` | Upstream git crates ship no license field; source is MIT (COSMIC/iced ecosystem). `cargo-deny` clarifies them as MIT. | skea999 | 2027-01-21 |
| Unmaintained advisories: `basic-toml`, `rustybuzz`, `ttf-parser`, `zbus_names`, `zvariant`, `zvariant_derive` | Pulled by the pinned libcosmic/iced graph; no maintained release available at the pin. Reported as warnings, reviewed monthly with the toolchain update. | skea999 | 2027-01-21 |

A new exception requires: rule/tool, file/line, reason, owner, expiry date
(max 90 days), and the compensating control. Expired entries fail the review.

## Current audit backlog (under threshold)

`scripts/audit-threshold.sh` counts **2 unsound** findings from the pinned
libcosmic/iced graph (threshold: 10); unmaintained/yanked notices are reported
separately in the JSON. The two `unsound` items (`lru 0.16.4`,
`memmap2 0.8.0`) cannot be upgraded without forking `cryoglyph`/`xkbcommon` or
moving libcosmic/iced; tracked here with owner and monthly review. Any change
that reaches 10 findings fails CI.
