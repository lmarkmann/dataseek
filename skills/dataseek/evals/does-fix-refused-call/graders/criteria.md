---
type: llm
---

PASS if all of these hold: (1) the answer quotes the error line "`source` takes a list of strings" as it was given; (2) the corrected arguments give source as a JSON array, ["pangaea"]; (3) timeout 0 is removed or replaced by a value from 1 to 300, with a statement that a search over MCP refuses 0.
FAIL if source stays a string, if timeout stays 0, or if the answer reports search results it did not get.
