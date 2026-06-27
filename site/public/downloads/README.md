# /downloads

Public trial artifacts served as static files (e.g. `rustpdf-delphi-<version>.zip`
and its `.sha256`). They are **build outputs**, not committed — the multi-platform
build box / CI populates this directory with:

    make delphi-dist-publish        # builds the cdylib per target, zips, copies here

Locally you can run the same target to test the download link.
