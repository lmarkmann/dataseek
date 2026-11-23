---
type: llm
---

PASS if all of these hold: (1) the answer gives a JSON server entry whose command is dsk, an absolute path to dsk, or uvx, and whose args contain mcp; (2) it says the FRED key goes in credentials.toml in the dataseek config directory or in the environment the server starts in; (3) no literal key value appears in the entry args, in a tool argument or on a command line.
FAIL if the answer invents a key flag or key argument such as --api-key, writes an assignment with a literal value such as FRED_API_KEY=abc123, or gives no server entry.
