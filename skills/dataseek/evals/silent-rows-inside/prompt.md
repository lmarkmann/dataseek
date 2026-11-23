---
name: silent-rows-inside
description: "rows inside one dataset belong to the archive's own API"
tags: [boundary]
plugins: ["../.."]
max_turns: 6
allowed_tools: [Read, Glob, Grep, Skill]
---

Load the first 100 rows of the nyu-mll/glue dataset from Hugging Face and show me the label distribution.
