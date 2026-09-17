#!/usr/bin/env node
// Distributor tooling only. This file is not bundled with the desktop app.
import { createPrivateKey, createPublicKey, sign } from 'node:crypto'
import { readSync } from 'node:fs'
import { resolve } from 'node:path'
import { pathToFileURL } from 'node:url'

function privateKey(pem) {
  let key
  try {
    key = createPrivateKey({ key: pem, format: 'pem' })
  } catch {
    throw new Error('Could not read an Ed25519 private PEM key from stdin.')
  }
  if (key.asymmetricKeyType !== 'ed25519') {
    throw new Error('The signing key must be Ed25519.')
  }
  return key
}

export function verificationKey(pem) {
  const jwk = createPublicKey(privateKey(pem)).export({ format: 'jwk' })
  return Buffer.from(jwk.x, 'base64url').toString('base64')
}

export function issueLicense(pem, licenseId, issuedAt = Math.floor(Date.now() / 1000)) {
  if (typeof licenseId !== 'string' || licenseId.length < 1 || licenseId.length > 128 || /[^A-Za-z0-9_.:-]/.test(licenseId)) {
    throw new Error('License ID must contain 1–128 ASCII letters, digits, or -_.:.')
  }
  if (!Number.isSafeInteger(issuedAt) || issuedAt <= 0 || issuedAt > 253402300799) {
    throw new Error('Issuance time must be a positive Unix timestamp in seconds before year 10000.')
  }
  const payload = Buffer.from(JSON.stringify({
    version: 1,
    product: 'sculpt-desktop',
    licenseId,
    issuedAt,
  }))
  return {
    version: 1,
    payload: payload.toString('base64'),
    signature: sign(null, payload, privateKey(pem)).toString('base64'),
  }
}

function readKeyFromStdin() {
  // Bound input without ever writing private material to disk or command output.
  const buffer = Buffer.alloc(4097)
  let length = 0
  while (length < buffer.length) {
    const read = readSync(0, buffer, length, buffer.length - length)
    if (read === 0) break
    length += read
  }
  if (length === 0 || length > 4096) {
    throw new Error('Supply one private PEM key, up to 4 KB, on stdin.')
  }
  return buffer.subarray(0, length).toString('utf8')
}

const usage = `Distributor-only offline Sculpt license issuer.

  node scripts/issue-license.mjs --public-key < signing-key.pem
  node scripts/issue-license.mjs --license-id UNIQUE-ID [--issued-at UNIX-SECONDS] < signing-key.pem > customer.sculpt-license

The private key is supplied by the distributor on stdin. This tool does not
generate keys, store them, contact a server, or confirm a payment.`

function main(args) {
  if (args.length === 1 && args[0] === '--help') {
    process.stdout.write(`${usage}\n`)
    return
  }
  if (args.length === 1 && args[0] === '--public-key') {
    process.stdout.write(`${verificationKey(readKeyFromStdin())}\n`)
    return
  }
  const options = new Map()
  for (let i = 0; i < args.length; i += 2) {
    const name = args[i]
    const value = args[i + 1]
    if (!['--license-id', '--issued-at'].includes(name) || options.has(name) || value === undefined) {
      throw new Error(`Invalid arguments.\n${usage}`)
    }
    options.set(name, value)
  }
  if (!options.has('--license-id')) throw new Error(`A license ID is required.\n${usage}`)
  const timestamp = options.get('--issued-at')
  if (timestamp !== undefined && (timestamp.length === 0 || /[^0-9]/.test(timestamp))) {
    throw new Error('Issuance time must be an integer Unix timestamp in seconds.')
  }
  const envelope = issueLicense(
    readKeyFromStdin(),
    options.get('--license-id'),
    timestamp === undefined ? undefined : Number(timestamp),
  )
  process.stdout.write(`${JSON.stringify(envelope, null, 2)}\n`)
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    main(process.argv.slice(2))
  } catch (error) {
    process.stderr.write(`${error instanceof Error ? error.message : 'License issuance failed.'}\n`)
    process.exitCode = 1
  }
}
