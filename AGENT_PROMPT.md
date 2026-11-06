# Kickoff prompt for coding agent on zoro

You are implementing **lilvault** on zoro. Read `SPEC.md` and `TASKS.md` in full. Follow the spec's fixed decisions and security requirements. Build the smallest reliable end-to-end implementation: existing Obsidian Sync vault -> official Headless client in Docker -> Rust MCP gateway -> Hermes agent. Multiple vaults must be supported, but the first demo can use one disposable vault.

**Begin with M0, not Rust application code.** Inspect the host directly when already on zoro; use SSH only when running elsewhere. Inspect official Headless CLI documentation/installed behavior. Report findings, plan commands, and any required approvals. Validate real bidirectional synchronization and restart persistence before proceeding to M1. Do not assume a public hostname or alter host networking without approval. Do not store credentials in Git or logs.

After M0 passes, implement M1 and M2 as a vertical slice. Maintain TASKS.md, commit logical milestones, and report verification results. Prefer working code and tests over architecture speculation. If blocked by proprietary Sync behavior, report the blocker rather than implementing an unofficial protocol.

At each milestone provide: what changed, commands/tests run, outcomes, remaining risks, and next steps. Success requires a real Hermes MCP operation and an observed Obsidian Sync round trip.
