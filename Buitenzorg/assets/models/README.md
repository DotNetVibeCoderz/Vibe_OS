# Language models on the image

`stories260K.gguf` -- the 260K-parameter TinyStories model from Andrej
Karpathy's [llama2.c](https://github.com/karpathy/llama2.c) (MIT licence),
converted to GGUF by the llama.cpp project and published at
`huggingface.co/ggml-org/models` (`tinyllamas/stories260K.gguf`,
SHA-256 `270cba1bd5109f42d03350f60406024560464db173c0e387d91f0426d3bd256d`).
It goes onto every image as `/disk/STORIES.GGU` and is what `llm` and the
LLM self-test run. Bigger models (for example `stories15M-q4_0.gguf`, 19 MB,
same place) can be downloaded with Unduhan and run with
`llm /disk/STORY15M.GGU Once upon a time`.
