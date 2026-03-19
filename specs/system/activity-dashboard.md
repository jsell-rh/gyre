# Activity Dashboard / Feed

- **Real-time activity feed** showing what agents and users have been working on over any time period.
- Each activity entry links to the relevant artifact - PR, commit, branch, test results, etc.
- **"Try it" links:** For completed work, provide a way for users to spin up an ephemeral environment (NixOS + Tailscale) running that version of the code, so they can interact with and test what was built - without touching production.
- This is the human window into the Ralph loop: see what's happening, attach to a TTY if needed, spin up a preview, give feedback.
- Feeds the analytics system: what did users actually try? What did they approve or reject?
