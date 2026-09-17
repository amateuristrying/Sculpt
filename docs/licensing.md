# Offline application licensing

Sculpt sells access to its desktop software and local runtime management. Image
uploads, inference, and generated assets remain on the user's machine. The
licensing boundary is separate from the inference harness's runtime adapters;
neither generation nor verification calls a Sculpt GPU server.

## Implemented policy

- Development builds allow unlimited reconstruction so engineering and image
  benchmarks do not consume a trial.
- Release builds allow one successful real reconstruction before activation.
  An asset must be durably saved before the store consumes that trial. Failed
  jobs, cancellations, and procedural demos do not consume it.
- A valid signed perpetual license permits unlimited local reconstruction.
  Existing project inspection and export should remain available after a trial.
- The native backend decides access when admitting a job. The frontend displays
  that decision and cannot supply a trusted `paid` flag or reset the trial.

`LicensePolicy::configured()` uses Rust's `debug_assertions` build configuration
and the compile-time `SCULPT_LICENSE_PUBLIC_KEY` environment variable. This is a
**public**, standard-base64 Ed25519 verification key containing exactly 32 bytes.
It is safe to distribute with the application. A release without a valid key
still offers its trial and explicitly reports that activation is unavailable.
Production distribution must configure a key and an actual way to obtain a
license; this prototype does not provide checkout or collect money.

The application operation lock must cover access checking and job registration.
The durable store must commit the successful asset and consumed-trial state
together before releasing that lock. A crash during inference should leave the
trial available. This avoids spending the trial on an error or letting concurrent
jobs claim the same free use.

## License envelope version 1

Activation accepts UTF-8 JSON of the following shape, with no additional fields:

```json
{
  "version": 1,
  "payload": "BASE64_OF_EXACT_SIGNED_JSON_BYTES",
  "signature": "BASE64_OF_64_BYTE_ED25519_SIGNATURE"
}
```

The decoded payload is a JSON object with exactly these claims:

```json
{
  "version": 1,
  "product": "sculpt-desktop",
  "licenseId": "a-unique-license-identifier",
  "issuedAt": 1789430400
}
```

`issuedAt` is a positive Unix timestamp in seconds, bounded to year 9999. It is
issuance metadata, not an expiration time. Identifiers contain 1–128 ASCII
letters, digits, or `-_.:`. Envelopes are limited to 16 KB and decoded payloads to
4 KB. Standard padded base64 is required. Unsupported versions, unknown or
duplicate fields, invalid claims, weak build keys, and invalid signatures are
rejected.

The issuer serializes claims once, signs those **exact bytes** with its private
Ed25519 key, and encodes the bytes and signature into the envelope. No particular
JSON key order or whitespace is required, because verification uses the original
decoded bytes instead of re-serializing the object. Sculpt verifies with
`ed25519-dalek` strict verification, then parses the verified claims. It validates
an activation before storage and verifies stored licenses again before granting
access. Deterministic signing keys in unit tests are test fixtures only and are
not included in the production binary.

The real signing key must stay with a distributor-controlled issuer. Never add
it to the repository, frontend, runtime installer, application bundle, or public
release environment. Only the public key is passed when compiling Sculpt, for
example `SCULPT_LICENSE_PUBLIC_KEY='<public key>' npm run desktop:build`.
Changing the configured public key requires rebuilding
the Rust application and invalidates licenses signed by the previous key; a
future key-rotation format can carry a trusted key identifier.

The distributor-only `scripts/issue-license.mjs` uses Node's built-in Ed25519
implementation and is not included in the desktop bundle. It takes a private PEM
key on stdin; it does not create keys or write private material. Given a securely
managed signing key, these commands derive its public key and issue a license:

```sh
node scripts/issue-license.mjs --public-key < signing-key.pem
node scripts/issue-license.mjs --license-id YOUR-UNIQUE-LICENSE-ID < signing-key.pem > customer.sculpt-license
```

The first output configures the release build. The second output can be delivered
to a customer for offline activation after your external purchase process has
verified their payment. Neither command verifies a purchase. Do not place the
private PEM key in this repository; the paths above are examples of issuer input.
Run issuer tests with `node --test scripts/issue-license.test.mjs`; their keys are
ephemeral and never written to disk.

## Practical limits and next integration

This is a functional offline verification boundary for internal prelaunch builds,
not a production-ready commercial release. Checkout, payment verification/webhooks,
secure production license issuance,
customer delivery and recovery, refunds, and signed/notarized distribution still
need to be integrated. No checkout URL or pretend payment success is built into
this implementation.

The local trial record is **not tamper-proof**. Someone controlling their machine
can delete application data, replace the application, or patch its code. A signed
license prevents forging a paid entitlement for an unmodified build; it does not
make an offline trial impossible to reset. This first format also has no device
binding, online revocation, or device-count enforcement, so license files can be
copied. Those are explicit product decisions to make before commercial launch.
An online purchase or optional activation service can issue licenses later
without becoming a per-generation inference dependency.
