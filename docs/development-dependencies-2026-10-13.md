# Development dependency update — 2026-10-13

Vitest is pinned at 4.1.11 and source-map-js resolves to 1.2.2. The former fixes
[the Vitest mock-path advisory](https://github.com/vitest-dev/vitest/security/advisories/GHSA-82fw-gwwq-j7x9)
and removes the old tinypool dependency from this lockfile. These are development
tools; the Python model/runtime lock and reconstruction settings are unchanged.

All added or version-changed packages were checked against their package
metadata in the resolved lockfile:

| Packages | License |
|---|---|
| Vitest and @vitest/expect, mocker, pretty-format, runner, snapshot, spy, utils | MIT |
| @standard-schema/spec, chai, es-module-lexer, obug, std-env, tinyexec, tinyrainbow | MIT |
| source-map-js | BSD-3-Clause |

No model weights or inference dependencies were added. `npm audit` reports zero
known advisories for this resolved tree at the time of the update; this is a
point-in-time advisory check, not a security certification. Unit tests pass with
the upgraded runner. Node 22 is used in CI.
