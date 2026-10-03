#!/usr/bin/env python3
"""Exercise real JDK signing and failure cases using disposable credentials."""

import os
from pathlib import Path
import secrets
import subprocess
import tempfile
import unittest
from zipfile import ZipFile


SIGNER = Path(__file__).resolve().with_name('sign_android_bundle.sh')


class BundleSigningTest(unittest.TestCase):
    def test_signing_integrity_and_input_guards(self):
        with tempfile.TemporaryDirectory(prefix='babytrack-signing-test-') as directory:
            root = Path(directory)
            key = root / 'test.p12'
            password = secrets.token_urlsafe(24)
            env = dict(os.environ, BABYTRACK_UPLOAD_KEYSTORE=str(key),
                       BABYTRACK_UPLOAD_ALIAS='upload',
                       BABYTRACK_UPLOAD_STORE_PASSWORD=password,
                       BABYTRACK_UPLOAD_KEY_PASSWORD=password)
            subprocess.run([
                'keytool', '-genkeypair', '-keystore', str(key), '-storetype', 'PKCS12',
                '-alias', 'upload', '-keyalg', 'RSA', '-keysize', '2048',
                '-validity', '365', '-dname', 'CN=Disposable Signing Test',
                '-storepass:env', 'BABYTRACK_UPLOAD_STORE_PASSWORD',
                '-keypass:env', 'BABYTRACK_UPLOAD_KEY_PASSWORD',
            ], env=env, check=True, capture_output=True)
            unsigned = root / 'unsigned.aab'
            signed = root / 'signed.aab'
            with ZipFile(unsigned, 'w') as bundle:
                for abi in ('arm64-v8a', 'x86_64'):
                    bundle.writestr(f'base/lib/{abi}/libbabytrack_core_ffi.so', b'fixture')
                bundle.writestr('base/manifest/AndroidManifest.xml', b'fixture')

            def sign(source, destination):
                return subprocess.run(['bash', str(SIGNER), str(source), str(destination)],
                                      env=env, capture_output=True, text=True)

            result = sign(unsigned, signed)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            original = signed.read_bytes()
            self.assertTrue(Path(str(signed) + '.pem').exists())
            self.assertTrue(Path(str(signed) + '.sha256').exists())
            self.assertNotEqual(sign(unsigned, signed).returncode, 0)
            self.assertEqual(signed.read_bytes(), original, 'Existing output was changed')
            self.assertNotEqual(sign(signed, root / 'resigned.aab').returncode, 0)
            self.assertFalse((root / 'resigned.aab').exists())

            with ZipFile(unsigned, 'a') as bundle:
                bundle.writestr('base/lib/x86/dependency-only.so', b'fixture')
            self.assertNotEqual(sign(unsigned, root / 'bad-abi.aab').returncode, 0)
            self.assertFalse((root / 'bad-abi.aab').exists())

            # JDK strict verification must reject an unsigned entry added after signing.
            with ZipFile(signed, 'a') as bundle:
                bundle.writestr('base/assets/tampered.txt', b'unsigned addition')
            verify = subprocess.run([
                'jarsigner', '-verify', '-strict', '-keystore', str(key),
                '-storepass:env', 'BABYTRACK_UPLOAD_STORE_PASSWORD', str(signed), 'upload',
            ], env=env, capture_output=True)
            self.assertNotEqual(verify.returncode, 0)


if __name__ == '__main__':
    unittest.main()
