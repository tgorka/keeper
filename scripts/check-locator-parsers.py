#!/usr/bin/env python3
"""Hold the shared locator vectors to the real parsers.

`keeper-graph-protocol/src/locator.rs` admits a `$locator` string only when it matches its own
allowlist grammars. That grammar must never be more permissive than the parsers a resolver would
hand the string to. For every `locators[]` entry of `syncular/knowledge-graph.vectors.json`, this
runs `locator`, `selector` and `credential_ref` through

- the installed libpq's `PQconninfoParse` (keyword DSNs and `postgres://` URIs), and
- Node's WHATWG `URL`, both absolute and relative to a base (userinfo, query and fragment keys),

and fails when

- an ADMITTED vector yields a credential in either parser, or
- a vector in `CONFIRMED` (a known bypass) is admitted or yields no credential in either parser
  (so each listed refusal is a real attack, not a strawman).

Fail-closed: no libpq or no node is an error, never a skip. `KEEPER_LIBPQ` and `KEEPER_NODE`
override the lookup.
"""
import ctypes
import ctypes.util
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
VECTORS = ROOT / 'syncular/knowledge-graph.vectors.json'
FIELDS = ('locator', 'selector', 'credential_ref')
# libpq options that carry a secret.
LIBPQ_SECRETS = {'password', 'sslpassword', 'oauth_client_secret'}
# URL query/fragment keys that carry a credential (locator.rs CREDENTIAL_PARAMETERS and
# DSN_PASSWORD_KEYS), and the credential-name suffixes of CREDENTIAL_KEYS.
URL_SECRETS = {'token', 'access_token', 'password', 'secret', 'sig', 'signature',
               'x-amz-signature', 'x-amz-credential', 'x-amz-security-token',
               'x-goog-signature', 'pwd', 'passwd', 'sslpassword'}
SECRET_SUFFIX = re.compile(r'(password|passwd|pwd|secret|token|apikey|privatekey|secretkey|'
                           r'accesskey|credentials?|authorization|bearer)$')
# Refused vectors a real parser reads a credential from. Append-only, like the vectors.
CONFIRMED = {
    # 111.1 r1 B2: libpq whitespace forms.
    'dsn-password-pair', 'dsn-password-spaced', 'dsn-password-space-before',
    'dsn-password-space-after', 'dsn-password-first',
    'dsn-password-quoted-value', 'dsn-password-after-quoted-value', 'dsn-sslpassword',
    'dsn-password-in-selector',
    # URL userinfo and query forms.
    'userinfo-password', 'userinfo-user-only', 'userinfo-in-selector', 'dsn-userinfo',
    'query-token', 'query-access_token', 'query-password', 'query-secret', 'query-sig',
    'query-signature',
    'query-x-amz-signature', 'query-x-amz-credential', 'query-x-amz-security-token',
    'query-x-goog-signature', 'query-percent-encoded', 'fragment-access-token',
    'url-query-password-upper', 'url-query-password-spaced', 'url-query-sslpassword',
    'url-query-pwd-encoded',
    'url-scheme-relative-userinfo', 'url-upper-scheme-userinfo',
    # 111.1 r2 B2a: a key straight after a closing quote.
    'dsn-quoted-adjacent-password', 'dsn-empty-quoted-adjacent-password',
    'dsn-quoted-adjacent-sslpassword', 'dsn-quoted-adjacent-in-selector',
    # 111.1 r2 B2b: whitespace inside URL userinfo.
    'url-userinfo-space', 'url-userinfo-nbsp',
    # 111.1 r2: further real-parser credential forms the grammar refuses.
    'url-triple-slash-userinfo', 'url-query-at-userinfo', 'url-fragment-at-userinfo',
    'url-query-value-at-userinfo', 'url-fragment-value-at-userinfo',
    'dsn-oauth-client-secret', 'url-query-plus-key', 'url-query-api-key',
    'credential-ref-userinfo',
}


def libpq():
    names = [os.environ.get('KEEPER_LIBPQ'), ctypes.util.find_library('pq'), 'libpq.so.5',
             '/opt/homebrew/opt/libpq/lib/libpq.dylib', '/usr/local/opt/libpq/lib/libpq.dylib']
    for name in filter(None, names):
        try:
            lib = ctypes.CDLL(name)
        except OSError:
            continue

        class Option(ctypes.Structure):
            _fields_ = [('keyword', ctypes.c_char_p), ('envvar', ctypes.c_char_p),
                        ('compiled', ctypes.c_char_p), ('val', ctypes.c_char_p),
                        ('label', ctypes.c_char_p), ('dispchar', ctypes.c_char_p),
                        ('dispsize', ctypes.c_int)]

        lib.PQconninfoParse.restype = ctypes.POINTER(Option)
        lib.PQconninfoParse.argtypes = [ctypes.c_char_p, ctypes.POINTER(ctypes.c_char_p)]
        lib.PQconninfoFree.argtypes = [ctypes.POINTER(Option)]
        lib.PQfreemem.argtypes = [ctypes.c_void_p]

        def parse(text):
            error = ctypes.c_char_p()
            options = lib.PQconninfoParse(text.encode(), ctypes.byref(error))
            if not options:
                if error:
                    lib.PQfreemem(error)
                return None
            values = {}
            i = 0
            while options[i].keyword:
                if options[i].val is not None:
                    values[options[i].keyword.decode()] = options[i].val.decode('utf-8', 'replace')
                i += 1
            lib.PQconninfoFree(options)
            return values

        return parse
    raise SystemExit('check-locator-parsers: libpq not found (set KEEPER_LIBPQ)')


NODE_SCRIPT = r"""
const texts = JSON.parse(require('fs').readFileSync(0, 'utf8'));
const out = texts.map((text) => {
  const seen = [];
  for (const base of [undefined, 'https://base.invalid/dir/']) {
    let url;
    try { url = base === undefined ? new URL(text) : new URL(text, base); } catch { continue; }
    const keys = [...url.searchParams.keys()];
    if (url.hash) keys.push(...new URLSearchParams(url.hash.slice(1)).keys());
    seen.push({ base: base ?? null, username: url.username, password: url.password, keys });
  }
  return seen;
});
process.stdout.write(JSON.stringify(out));
"""


def node(texts):
    exe = os.environ.get('KEEPER_NODE') or shutil.which('node')
    if not exe:
        raise SystemExit('check-locator-parsers: node not found (set KEEPER_NODE)')
    run = subprocess.run([exe, '-e', NODE_SCRIPT], input=json.dumps(texts), text=True,
                         capture_output=True, check=True)
    return json.loads(run.stdout)


def url_secret(key):
    folded = key.strip().lower()
    bare = folded.replace('_', '').replace('-', '')
    return folded in URL_SECRETS or bool(SECRET_SUFFIX.search(bare))


def main():
    corpus = json.loads(VECTORS.read_text())
    entries = corpus['locators']
    ids = [e['id'] for e in entries]
    missing = CONFIRMED - set(ids)
    if missing:
        raise SystemExit(f'check-locator-parsers: CONFIRMED ids not in the vectors: '
                         f'{sorted(missing)}')
    rows = []
    for entry in entries:
        try:
            envelope = json.loads(entry['json'])
        except ValueError:
            envelope = None
        texts = [envelope[f] for f in FIELDS
                 if isinstance(envelope, dict) and isinstance(envelope.get(f), str)]
        rows.append((entry, texts))
    flat = [t for _, texts in rows for t in texts]
    parsed = iter(node(flat))
    pq = libpq()
    failures = []
    confirmed = 0
    for entry, texts in rows:
        findings = []
        for text in texts:
            values = pq(text)
            if values:
                findings += [f'libpq {k}' for k in sorted(LIBPQ_SECRETS) if values.get(k)]
            for seen in next(parsed):
                where = 'url' if seen['base'] is None else 'url(relative)'
                if seen['username'] or seen['password']:
                    findings.append(f'{where} userinfo')
                findings += [f'{where} key {k!r}' for k in seen['keys'] if url_secret(k)]
        admitted = 'refusal' not in entry
        listed = entry['id'] in CONFIRMED
        if admitted and findings:
            failures.append(f"{entry['id']}: admitted, but a real parser reads {findings}")
        if listed and admitted:
            failures.append(f"{entry['id']}: listed as a confirmed bypass, but admitted")
        if listed and not findings:
            failures.append(f"{entry['id']}: listed as a confirmed bypass, but no real parser "
                            'reads a credential')
        if findings and not admitted and not listed:
            failures.append(f"{entry['id']}: refused and a real parser reads {findings}: "
                            'pin it in CONFIRMED')
        confirmed += bool(findings) and not admitted
    for line in failures:
        print('FAIL', line)
    print(f'check-locator-parsers: {len(entries)} vectors, '
          f'{sum("refusal" not in e for e in entries)} admitted with no real-parser credential, '
          f'{confirmed} refused with one ({len(CONFIRMED)} pinned)')
    return 1 if failures else 0


if __name__ == '__main__':
    sys.exit(main())
