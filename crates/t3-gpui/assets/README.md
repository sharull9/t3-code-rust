# Bundled interface assets

Geist and Geist Mono are from [Vercel's Geist font v1.7.2](https://github.com/vercel/geist-font/releases/tag/v1.7.2), commit `a73329da8fc62afc917f796555202e4997f79b7c`. The files come from `fonts/Geist/ttf` and `fonts/GeistMono/ttf`. Regular, Medium, SemiBold, Bold, Italic and BoldItalic are included for each family. They are registered before component initialization and embedded in the executable. Redistribution terms are in [fonts/LICENSE.txt](fonts/LICENSE.txt), the upstream SIL Open Font License 1.1.

Provider marks come from [SVG Logos](https://svglogos.dev/), downloaded on 2026-10-02:

| Provider | Source |
| --- | --- |
| Codex | https://cdn.svglogos.dev/logos/codex.svg |
| Claude | https://cdn.svglogos.dev/logos/claude-icon.svg |
| Cursor | https://cdn.svglogos.dev/logos/cursor-icon.svg |
| OpenCode | https://cdn.svglogos.dev/logos/opencode-icon.svg |
| Antigravity | https://cdn.svglogos.dev/logos/antigravity.svg |
| Grok | https://cdn.svglogos.dev/logos/grok.svg |
| Gemini | https://cdn.svglogos.dev/logos/google-gemini-icon.svg |

Monochrome marks are tinted to match their native context. Antigravity, Gemini and OpenCode use the full-color SVG image renderer to preserve gradients and interior details. `opencode-icon-dark.svg` derives from the original with its two fill colors changed for dark backgrounds. Provider logos remain the property of their respective owners. Unknown drivers show initials until a matching logo is added.

All files are embedded; opening the app does not fetch fonts or logos.
