# The binary transcribes; the agent extracts lessons

`scribe` only fetches sources and writes transcripts. It never calls an LLM. Extraction of lessons happens in the running agent through the skill, one helper per source. Reasons: no API keys or prompt maintenance inside the binary, lesson quality comes from the user's main model, and a fresh helper context per source keeps long transcripts out of the main agent's context.
