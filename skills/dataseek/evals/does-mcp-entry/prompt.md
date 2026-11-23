---
name: does-mcp-entry
description: "the Setup lane's Config part, with a key that must not travel in the entry"
tags: [train]
plugins: ["../.."]
max_turns: 20
allowed_tools: [Read, Glob, Grep, Skill, Write, Edit, Bash]   # Write/Edit/Bash also need the operator's --allow-tools
---

Write the MCP server entry for dataseek for my client's config file. I also have a FRED key, where do I put it?
