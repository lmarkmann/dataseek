---
name: reads-cli-fallback
description: "no tools listed: the fallback rule lives in the MCP reference"
tags: [validation]
plugins: ["../.."]
max_turns: 6
allowed_tools: [Read, Glob, Grep, Skill]
---

The dataseek tools are not in my tool list, but dsk is installed. Give me the exact command that searches Zenodo and Figshare for sea ice extent and returns ten results I can parse.
