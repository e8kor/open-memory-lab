---
name: issue-agent-key
description: Use when the operator asks to issue, create or set up an Open Memory Lab key for an agent, such as the key the oml plugin reads from OML_API_KEY. Runs the oml CLI's issue-agent verb with --out, so the key value goes to an owner-only file and never into this conversation.
---

# Issue an agent key without seeing it

This skill issues the one key an agent needs on an Open Memory Lab server: a writer key with a
writer grant and a reader grant on one workarea, which is also the key's default workarea. It runs
the `oml` CLI on this machine, under the server and admin key the CLI already uses, and the server
journals the issuance like any other.

The key value must never enter this conversation. The CLI writes it to a file only the operator's
user can read and prints two lines that carry the file's path, not the value. Every rule below
follows from that.

## Rules

- Run `issue-agent` only with `--out`, and never with `--json`: the JSON response carries the value.
- Never read, print or copy the key file, and never run the two printed lines yourself.
- Never run another command whose output carries a key value: `oml keys issue` and
  `oml bootstrap claim` print one.
- Never pass `--key` on a command line. The CLI reads the admin key from `OML_CLI_KEY` or from the
  credentials `oml bootstrap claim` stored; if it finds none, tell the operator and stop.
- Keep the key file outside every repository.

## Steps

1. Ask the operator for anything they have not given:
    - the **principal** the key acts as, for example `crew/agent-7`. Give each agent its own:
      review independence and the reciprocal-approval statistics key on it.
    - the **workarea id**, the UUID of the workarea the agent will write and recall in.
    - whether a person rather than an agent holds the key (`--held-by human`).
    - whether the key must not read promoted memory on that workarea (`--write-only`). Without
      it the CLI adds a reader grant beside the writer grant; say so before you run it.

2. Choose the key file: `$HOME/.config/oml/keys/<principal>`, with each `/` of the principal
   written as `-`, so `crew/agent-7` becomes `crew-agent-7`. If the operator names another path,
   it must be outside any repository. The CLI refuses a path that already exists or holds a quote
   or a newline; ask for another name rather than removing a file.

3. Create the directory, owner-only:

    ```sh
    mkdir -p "$HOME/.config/oml/keys" && chmod 700 "$HOME/.config/oml/keys"
    ```

4. Run the issuance, adding `--held-by human` or `--write-only` only if the operator asked:

    ```sh
    oml keys issue-agent --principal crew/agent-7 --workarea <workarea-id> --out "$HOME/.config/oml/keys/crew-agent-7"
    ```

    On success stdout holds two `export` lines and stderr names the key id, its grants and its
    default workarea. Exit `1` is the server refusing the issuance, with the reason and the rule
    it names, and nothing was issued. Exit `2` is a local failure: a refusal such as an existing
    file or no server configured, where nothing was issued, or one of the two cases at the end of
    this page. Report what the CLI printed and stop; never retry without `--out`.

5. Give the operator the two printed lines exactly as the CLI printed them, to run in the shell
   that will start the agent, together with the key id from stderr. They look like this:

    ```sh
    export OML_URL='https://oml.example.com'
    export OML_API_KEY="$(cat '/home/op/.config/oml/keys/crew-agent-7')"
    ```

6. Tell the operator to delete the key file once the key is in that environment: the file is the
   only copy of the value, and the server keeps only its hash.

If the CLI says the key was issued but its value could not be written, the value is lost and the
key is live. Pass on the key id and the remedy the CLI prints, and issue a new key to a new file.

If the CLI says a key for the principal may have been issued, because no usable answer came back,
it cannot tell whether that key exists, and if it does its value is lost. Pass on what the CLI
printed, and issue a new key to a new file: the operator holds no usable key either way.
