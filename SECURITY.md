# Security Policy

## Reporting a Vulnerability

If you discover a security vulnerability in Threader, please report it responsibly.

**Email:** [security@continue.dev](mailto:security@continue.dev)

### What to include

- Description of the vulnerability
- Steps to reproduce
- Potential impact
- Suggested fix (if any)

### Response Timeline

- **Acknowledgment:** Within 48 hours of your report
- **Status update:** Within 5 business days
- **Resolution:** Depends on severity; critical issues are prioritized

### Scope

The following are in scope for security reports:

- The Threader daemon (`threader` binary)
- Authentication and credential storage
- Session data encryption and transmission
- The install script (`install.sh`)

### Out of Scope

- The hosted dashboard at threader.sh (report separately to security@continue.dev with "threader.sh" in the subject)
- Social engineering attacks
- Denial of service attacks

## Disclosure Policy

We follow coordinated disclosure. Please do not publicly disclose vulnerabilities until we have released a fix and confirmed it is safe to disclose.
