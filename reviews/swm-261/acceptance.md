# SWM-261 acceptance evidence

`src/bin/swm261_acceptance.rs` is a read-only source collector. It backs up the local ledger through SQLite's read-only online backup API, migrates and exercises the writer only on temporary copies, and uses read-only relay and Prometheus requests.

Live-source observations and proof records are kept on the SWM-261 Linear issue. This repository contains no live ledger, relay, lane, or metrics output.

The checked-in verification for this change is the bounded-page ledger test, the relay event parser tests, all-target compilation and clippy, and formatting. The copy-only live proof records page advancement, row matching, and historical coverage without altering the source ledger.
