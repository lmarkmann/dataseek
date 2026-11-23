---
name: does-fix-refused-call
description: "the Troubleshoot lane on a refused call with two wrong arguments"
tags: [validation]
plugins: ["../.."]
max_turns: 20
allowed_tools: [Read, Glob, Grep, Skill, Write, Edit, Bash]   # Write/Edit/Bash also need the operator's --allow-tools
---

I called the dataseek search tool with {"query": "ocean heat content", "source": "pangaea", "timeout": 0} and got:

Error: `source` takes a list of strings
  Try:   tools/list shows the type of each argument

Fix my call.
