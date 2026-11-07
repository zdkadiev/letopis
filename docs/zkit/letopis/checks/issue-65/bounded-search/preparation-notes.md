# Preparation failures and scope

These are not ordinary measurements or product regression findings.

- First SQLite backup attempt using the default Python returned OperationalError:
  unable to open database file. Only an empty owned destination had been created;
  original had no WAL and was quiescent. Retrying source immutable read-only backup
  succeeded; both owned databases passed integrity_check. Original was never reset.
- First conditioning launcher used the default Python3.9 and failed on the existing
  Python3.10+ union annotation before target launch. Python3.12 ran the recorded
  successful conditioning. No failed target timing was discarded.
- First SQL EXPLAIN capture produced CLI automatic display text despite -json.
  Original stdout/log remain in sql-plans/; explicit .explain off gives the eight
  validated structured captures in sql-plans-v2/. No product SQL was changed.
- Intermediate compile/lint diagnostics were fixed before actual-root gates.
  RED/hand-mutation logs intentionally fail; complete root make verify and native
  host manifest are the completion evidence. An old required-limit CLI test and
  old no-filter-before-limit test were corrected to the approved contract.
- Conditioning and two screening calls were predefined outside paired medians;
  every ordinary sample is retained. There was one completed ordinary campaign.
