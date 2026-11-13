# Agent instructions

## Web retrieval preference

Use TinyFish by default for web search and fetching pages when its tools are available. If unavailable in the current session, state that limitation and use another provider only when appropriate to the user’s request.

## Code exploration preference

Use Graphify first for code structure, architecture, callers, dependencies, and change impact. Check current local files for uncommitted or unindexed changes.
