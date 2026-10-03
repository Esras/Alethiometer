# pi-acp patches

Changes for [zboerner-qventus/pi-acp](https://github.com/zboerner-qventus/pi-acp) kept here until they land there. Each applies with `git apply` from the fork's root.

- `0001-gated-prompts-carry-kind-and-rawInput.patch` (against `c44f580`): a permission prompt shown on a gated tool call also carries that call's `kind` and `rawInput`, so a client that reads the request alone knows it gates a shell command, and which one. Alethiometer doesn't need it (it fills a request in from the tool call it already has, as ACP's `ToolCallUpdate` intends); other clients might. Adds one assertion to `test/component/session-permission-prompt.test.ts`; typecheck, lint, prettier and the 225 tests pass.
