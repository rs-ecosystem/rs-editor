# Design tokens

`tokens.json` adalah salinan token shadcn/ui ("Nova", palet neutral) dari Rs Organizer (`rs-organizer/design/tokens.json`), dengan skema yang sama: `light`, `dark`, `radius`, dan `font`. `rs-editor-theme` membaca file ini, sehingga editor dan aplikasi memakai warna, radius, dan font yang identik.

Untuk menyelaraskan dengan aplikasi lain, berikan `tokens.json` milik aplikasi tersebut ke `Theme::from_tokens_json`. File di sini hanya nilai bawaan.
