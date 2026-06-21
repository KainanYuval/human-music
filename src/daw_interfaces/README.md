# DAW interfaces

One crate per DAW under this directory. Each exposes project scanning and DAW-specific metadata; the verification engine consumes them via path dependencies.

| DAW | Crate | Status |
|-----|-------|--------|
| GarageBand | [`garageband/`](garageband/) | `.band` scan, MetaData.plist, xattr evidence |
