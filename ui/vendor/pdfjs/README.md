# pdf.js — vendored

`pdf.min.mjs` and `pdf.worker.min.mjs`, from
[pdf.js](https://github.com/mozilla/pdf.js) **6.3.289**, fetched from
`https://cdnjs.cloudflare.com/ajax/libs/pdf.js/6.3.289/`.

Copyright Mozilla Foundation, licensed under the **Apache License 2.0**. The
full notice is retained at the top of each file, which is what the licence
asks for.

## Why it is in the repository rather than fetched

Proton's content security policy is `script-src 'self'`. Nothing loads from a
CDN at run time, deliberately: an editor that needs the network to open a file
is an editor that stops working on a train.

## Why it is not imported at boot

Together these are 1.7MB, and a session that never opens a PDF should never
pay for them. `ui/document.js` imports the library on first use, which is also
why it is a separate module from `app.js`.

## Updating

Replace both files with the same version of each — the worker and the library
check that they match and refuse to run otherwise — and update the version
above.
