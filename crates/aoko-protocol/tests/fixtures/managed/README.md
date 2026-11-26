# Frozen managed-host fixtures

These are **evidence, not source**. Three files copied out of `Aoko/Core/` so that the
protocol tests can keep pinning what the managed host did after `Aoko/` is deleted.

| File | Copied from | Last commit touching it there | Blob at `011984b` | Bytes | Lines | SHA-256 at capture |
| --- | --- | --- | --- | ---: | ---: | --- |
| `GameStateClient.cs` | `Aoko/Core/GameStateClient.cs` | `f1a9f5f` | `21ffba8` | 66926 | 1677 | `02c5558dc90c054be022b1d9842be75a646655bdcf5b561b74647edfb7225e19` |
| `Profile.cs` | `Aoko/Core/Profile.cs` | `117e035` | `ffd7900` | 43430 | 977 | `f7948cf004e2bb4274c078952605719b9d2aaddea5fc67fda65bafc925c19ea3` |
| `Clicker.cs` | `Aoko/Core/Clicker.cs` | `b01a2b1` | `10c8a92` | 106214 | 3360 | `70d0cf294e3b368deeebf06f4026f621a0743442aa8ecaa369586c06369e8245` |

The copies were taken from the working tree at `011984b` and verified byte-identical to
the files they came from by SHA-256. `Aoko/` was the original location and still holds
them until sign-off deletes it; this directory is what survives that deletion.

Two things about the bytes, so nobody "fixes" them:

- The byte counts are the working-tree form at capture. `.gitattributes` normalises text
  to `eol=crlf` on checkout, so a fresh clone may present `Clicker.cs` with CRLF where
  the copy above was taken from an LF tree. `tests/frozen_managed_fixtures.rs` hashes
  content with carriage returns dropped for exactly that reason.
- No assertion reads a line ending, a byte count or a file size. They read text.

## These files must not be edited

A test that pins a string in one of these files is asking "does the host we abandoned
still say this?", which is a question about history. Editing a fixture to make an
assertion pass would change that question into a different one, and the suite would then
certify a host that never existed. If a pin needs to change, do one of these instead:

- change the **assertion**, and say why in the commit message; or
- delete the **test** and its fixture together, and say why the port no longer needs the
  evidence.

`tests/frozen_managed_fixtures.rs` enforces the rule: it pins a content hash per file, so
any character edit fails with the instruction above in the assertion message.

## How the tests read them

`include_str!`, not `read_to_string`. The evidence is compiled into the test binaries, so
a deleted or renamed fixture is a compile error rather than a panic inside
`read_to_string` on the sign-off commit. That is the failure mode these copies exist to
remove: a red test discovered at sign-off, after a build that looked fine.
