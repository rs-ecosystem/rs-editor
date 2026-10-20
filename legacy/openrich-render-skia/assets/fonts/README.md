# Shared editor fonts

Unmodified Liberation Sans and Mono 2.1.5, plus unmodified hinted Noto Sans Arabic and Noto Sans Devanagari for scripts Liberation does not cover.

Source: https://github.com/liberationfonts/liberation-fonts/releases/tag/2.1.5

Archive: https://github.com/liberationfonts/liberation-fonts/files/7261482/liberation-fonts-ttf-2.1.5.tar.gz

The included LICENSE applies to the Liberation fonts and LICENSE-Noto to the Noto fonts (both SIL Open Font License 1.1); they are separate from the Rust code license. Skia and the browser load these exact bytes so native input geometry does not depend on device font substitutions. Noto covers Arabic and Devanagari; the browser loads it through `unicode-range` faces of the same CSS families. System fallback fonts still handle other characters absent from both (emoji, CJK).

Noto source: https://github.com/notofonts/notofonts.github.io/tree/main/fonts (`NotoSansArabic/hinted/ttf`, `NotoSansDevanagari/hinted/ttf`)

## SHA-256

- `LiberationMono-Bold.ttf`: `bd62a0672d0b9b6710b01df434c80ad54fa5f0835207eb7b17b7a761463067bb`
- `LiberationMono-BoldItalic.ttf`: `79451f3c09fe25116098853b7a2ca6e2436220ccc11af022979adbcf195be130`
- `LiberationMono-Italic.ttf`: `605c01c711b44480a7508d349dfbf3264e81fa43d69e61cfa7d10b86e764c4d1`
- `LiberationMono-Regular.ttf`: `f2b83c763e8afd21709333370bed4774337fae82267937e2b5aea7e2fbd922c1`
- `LiberationSans-Bold.ttf`: `788abee4c806d660e8aee46689dd8540cd4bb98da03dcc9d171ce3efd99a9173`
- `LiberationSans-BoldItalic.ttf`: `698da70fc191cc5f33ad4d6d3fe830fe4624b898ea2e3169955928b7c491f1ee`
- `LiberationSans-Italic.ttf`: `e5bae5c4cde31f22142753855f4f8fb86da6ff39955ed3c0a11248b0d16948b0`
- `LiberationSans-Regular.ttf`: `76d04c18ea243f426b7de1f3ad208e927008f961dc5945e5aad352d0dfde8ee8`
- `NotoSansArabic-Bold.ttf`: `4e5462d2e8be880317b9f49b5b2da109ddb6a3563d91cc604b67f3535832a555`
- `NotoSansArabic-Regular.ttf`: `bdff3e5659d67e67def05b33f749683b9376ae819d65d3dd62ac4640b3aaef48`
- `NotoSansDevanagari-Bold.ttf`: `6a09c8d797cfc803d32cdc731e809424d74cbaff59f503de34ade421a08e5bc2`
- `NotoSansDevanagari-Regular.ttf`: `4e3c66638958c3e2ab5d37f47a8deb89fffeb7be9985c665a519bbc7ba762313`
