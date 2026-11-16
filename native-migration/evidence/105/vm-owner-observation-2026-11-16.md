# Owner VM observation — 2026-11-16

User-reported environment: UTM, macOS 27.2.1, arm64.
The owner installed Electron, prepared test data, downloaded the private feed,
and replaced the VM app's updater settings through Finder.

The private server served the existing 1.99.0 arm64 artifacts, whose SHA-512
checksums were verified against the original recorded feed before staging.

Observation reported by the owner: “it worked but it logged me out”.
This is not a passing sign-in-preservation result. Pending confirmation:
installed version, Install versus ordinary quit, retained song/setlist/theme,
any migration notice or Keychain prompt, migration report and application logs.
No diagnosis of the sign-out has been confirmed. Do not infer that the owner
accepted migration, that all data survived, or that macOS 13 was tested.

Investigation: local migration code records cloud-key rewrap failures in
`kairo-native-migration.json`. Session restoration must also be checked;
a sign-in screen alone does not identify a cryptographic migration failure.
No production code changed on the basis of this observation.

## Follow-up evidence from the owner

The owner confirmed the test song, setlist and theme survived, and a
warning asked them to sign in again. A screenshot of the migration marker
confirmed `ran: true`, `upgraded: true`, a dated backup, and five imported
view preferences. The cloud key and API.Bible cache key both failed with
`OS keychain is unavailable`. The recorded key step took 5.330292 ms.

This confirms failure in secure-key migration, but not its underlying OS
error: the code maps several distinct keyring errors to the same text.
Do not infer a locked keychain, denied authorization, missing item, or VM
limitation without further evidence. The screenshot's personal path is
not copied here. Version and install mode still need independent confirmation.
