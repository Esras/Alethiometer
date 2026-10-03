# Shell corpus

`corpus.jsonl` holds 387 shell commands from
[godspede/construct-auto-classifier](https://github.com/godspede/construct-auto-classifier)
at commit `881fb45a96af4126dc854c4b6b77d813d41f4510`, copied from its bench files
`bench/battery.jsonl`, `bench/holdout.jsonl` and `bench/real-cases.jsonl`. It keeps
every `bash` case (the file-tool cases are left out) and drops duplicate commands.

That project is licensed under the Apache License 2.0 (Copyright 2026 godspede; its
NOTICE file reads "construct-auto-classifier / Copyright 2026 godspede"). The commands
are redistributed here under that license, with the fields renamed as described below.

Each line has these fields:

- `command`: the command text, exactly as it appears in their bench.
- `label`: their `expect` verdict for the command.
- `category`: their `category` field.
- `source`: `<their file>#<their case id>`.

Their labels say whether the command would cause harm from a list they keep (destroying
data, leaking credentials, running remote code, changing the system, getting around the
gate). They don't say whether the command has side effects:

- `allow`: routine. This includes ordinary writes such as `rm -rf node_modules` and `git push`.
- `deny`: the gate refuses it.
- `escalate`: the gate asks a human.
- `ask`: the gate asks a human.

`agents::shell::tests::corpus_never_reads_only_on_unsafe` treats `deny`, `escalate` and
`ask` as unsafe. It fails if our parser calls any of those commands `read-only`. It
skips the ids in `DISCLOSES`, which they flag only for what the output reveals (secret
reads such as `cat .env` or `env | grep token`, and a prompt injection hidden in a
comment). Those commands have no side effects, so `read-only` is correct for them.
