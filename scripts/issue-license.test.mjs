import assert from 'node:assert/strict'
import { generateKeyPairSync, verify } from 'node:crypto'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import test from 'node:test'
import { issueLicense, verificationKey } from './issue-license.mjs'

// Ephemeral test keys never leave process memory or the child process stdin.
const keys = generateKeyPairSync('ed25519')
const pem = keys.privateKey.export({ format: 'pem', type: 'pkcs8' })

test('issuer signs the exact claims accepted by Sculpt', () => {
  const envelope = issueLicense(pem, 'test-license-123', 1789430400)
  const payload = Buffer.from(envelope.payload, 'base64')
  assert.equal(envelope.version, 1)
  assert.deepEqual(JSON.parse(payload), {
    version: 1, product: 'sculpt-desktop', licenseId: 'test-license-123', issuedAt: 1789430400,
  })
  assert.equal(verify(null, payload, keys.publicKey, Buffer.from(envelope.signature, 'base64')), true)
  assert.equal(verify(null, Buffer.from('tampered'), keys.publicKey, Buffer.from(envelope.signature, 'base64')), false)
  assert.equal(Buffer.from(verificationKey(pem), 'base64').length, 32)
  assert.equal(verificationKey(pem), Buffer.from(keys.publicKey.export({ format: 'jwk' }).x, 'base64url').toString('base64'))
})

test('issuer rejects invalid identifiers, timestamps, and signing keys', () => {
  for (const id of ['', 'x'.repeat(129), 'hello world', 'bad\nline', 'bad\n']) {
    assert.throws(() => issueLicense(pem, id, 1789430400))
  }
  for (const timestamp of [0, -1, 1.5, NaN, 253402300800]) {
    assert.throws(() => issueLicense(pem, 'test', timestamp))
  }
  assert.throws(() => issueLicense('invalid PEM', 'test', 1789430400))
  const ec = generateKeyPairSync('ec', { namedCurve: 'P-256' })
  assert.throws(() => issueLicense(ec.privateKey.export({ format: 'pem', type: 'pkcs8' }), 'test', 1789430400))
})

test('CLI accepts private input on stdin and emits only the signed license', () => {
  const result = spawnSync(process.execPath, [fileURLToPath(new URL('./issue-license.mjs', import.meta.url)),
    '--license-id', 'cli-test', '--issued-at', '1789430400'], { input: pem, encoding: 'utf8' })
  assert.equal(result.status, 0, result.stderr)
  assert.equal(result.stderr, '')
  assert.equal(result.stdout.includes('PRIVATE KEY'), false)
  const envelope = JSON.parse(result.stdout)
  assert.equal(JSON.parse(Buffer.from(envelope.payload, 'base64')).licenseId, 'cli-test')
  assert.equal(verify(null, Buffer.from(envelope.payload, 'base64'), keys.publicKey, Buffer.from(envelope.signature, 'base64')), true)
})

test('CLI fails closed on oversized input and duplicate arguments', () => {
  const script = fileURLToPath(new URL('./issue-license.mjs', import.meta.url))
  const oversized = spawnSync(process.execPath, [script, '--public-key'], { input: 'x'.repeat(4097), encoding: 'utf8' })
  assert.equal(oversized.status, 1)
  assert.equal(oversized.stdout, '')
  const duplicate = spawnSync(process.execPath, [script, '--license-id', 'first', '--license-id', 'second'], { input: pem, encoding: 'utf8' })
  assert.equal(duplicate.status, 1)
  assert.equal(duplicate.stdout, '')
})
