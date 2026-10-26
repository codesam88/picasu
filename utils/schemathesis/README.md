# Schemathesis tier-1 check

Runs a bounded single-operation sweep against a throwaway seeded backend and
compares findings to `baseline.yaml`.

Run:

    just fuzz-check

What it does:

- builds the backend and snapfab fixtures
- seeds a disposable instance (fresh dirs, index, restart, shares)
- runs schemathesis with the fixed 5-check oracle
- compares the JSON report to the accepted-findings baseline

Updating the baseline when a bug is fixed: remove the corresponding entry
from `baseline.yaml` and re-run; a fixed bug must make that finding disappear.

State lives in a temp dir removed on exit.
