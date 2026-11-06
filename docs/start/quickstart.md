---
status: Stable
verified: 2026-11-06
answers: "six commands that start a server, write a memory, show it held, approve it as a second identity and recall it"
---

# Quickstart: see a memory held, then approved

You start a server on your machine, write a memory as an agent and watch it stay invisible until a different key approves it. It takes six commands and needs Docker with compose v2 and the `oml` CLI ([install](install.md)). Time to run: Not measured yet.

1. Start a server, a workarea named `crew`, an agent key and a reviewer key.

    ```sh
    oml quickstart
    ```

    It writes the key files `oml/keys/agent` and `oml/keys/reviewer`. In the next commands, `A` stands for the agent's key and `R` for the reviewer's.

    ```sh
    export OML_URL=http://127.0.0.1:8080
    A=$(cat oml/keys/agent); R=$(cat oml/keys/reviewer)
    ```

2. Write a memory as the agent. The answer says the memory is `held`.

    ```sh
    OML_API_KEY=$A oml write "We deploy on Tuesdays; Friday deploys need two approvals."
    ```

3. Recall it as the agent. You get no hit: a held memory is invisible to every read, even its author's.

    ```sh
    OML_API_KEY=$A oml read --query "deploy Friday"
    ```

4. Recall it again and ask for your own unreviewed memories. Now the memory appears, marked unreviewed and `held`.

    ```sh
    OML_API_KEY=$A oml read --query "deploy Friday" --include-unreviewed
    ```

5. Approve it with the reviewer key, a different identity. Paste the artifact id from step 2 or 4. The server refuses this verdict from the agent's own key.

    ```sh
    OML_API_KEY=$R oml review approve --ids <artifact_id>
    ```

6. Recall it with the agent key. The memory is found, with no flag.

    ```sh
    OML_API_KEY=$A oml read --query "deploy Friday"
    ```

## What you saw

The write waited for a reviewer, and the reviewer had to be someone other than the author. Approval changed who could read the memory, not how it ranks. The cost is a delay between writing and recall; [why a write is held](../concepts/why-a-write-is-held.md) explains what that buys.

## Next

- Connect an agent: [Claude Code](../guides/claude-code.md), [claude.ai and your phone](../guides/claude-ai-and-mobile.md), [other tools](../guides/other-harnesses.md).
- Run the server for a team: [run your own server](run-your-own.md).
- Review a queue: [review held memories](../guides/reviewing.md).
- Remove the stack: `docker compose --project-directory oml down -v`.
