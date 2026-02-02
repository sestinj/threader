#!/usr/bin/env bash
#
# Generate an Ed25519 keypair for signing Threader releases.
#
# Outputs:
#   - Base64-encoded private key (store as RELEASE_SIGNING_KEY GitHub secret)
#   - Base64-encoded public key (hardcode in src/sync/updater.rs)
#
# Requires: Python 3 with PyNaCl (pip install pynacl)

set -euo pipefail

if ! python3 -c "import nacl" 2>/dev/null; then
    echo "Installing PyNaCl..."
    pip3 install pynacl
fi

python3 -c "
import base64
from nacl.signing import SigningKey

signing_key = SigningKey.generate()
verify_key = signing_key.verify_key

private_b64 = base64.b64encode(bytes(signing_key)).decode()
public_b64 = base64.b64encode(bytes(verify_key)).decode()

print()
print('=== Ed25519 Release Signing Keypair ===')
print()
print('PRIVATE KEY (store as RELEASE_SIGNING_KEY GitHub Actions secret):')
print(private_b64)
print()
print('PUBLIC KEY (hardcode in src/sync/updater.rs as RELEASE_PUBLIC_KEY_BASE64):')
print(public_b64)
print()
print('IMPORTANT: Keep the private key secret. Do not commit it to the repository.')
"
