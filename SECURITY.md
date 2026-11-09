# Security Policy

The APS project takes software security seriously. As a systems programming language with compile-time safety guarantees, ensuring the correctness and isolation of the AMA (Automated Memory Architecture) and runtime primitives is our top priority.

---

## Supported Versions

Only the latest release version on the `master` branch receives active security updates:

| Version | Supported          |
| ------- | ------------------ |
| 1.0.x   | :white_check_mark: |
| < 1.0   | :x:                |

---

## Reporting a Vulnerability

If you discover a security vulnerability (such as a memory safety bypass, unexpected buffer overrun, compiler crash leading to code execution, or undefined behavior in safe code):

1. **Do NOT report security vulnerabilities via public GitHub issues.**
2. Send an email with a detailed description, minimal reproducible `.aps` source code, and environment details to:
   **security@aps-lang.org** (or open a private GitHub Security Advisory).
3. Include the compiler commit hash (`aps --version`) and operating system.

### What to Expect:
* **Initial Response:** Within 48 hours acknowledging receipt of your report.
* **Triage & Assessment:** Within 5 business days detailing whether the issue is reproducible and our remediation timeline.
* **Coordinated Disclosure:** We will work with you to test fixes and coordinate public disclosure alongside a security patch release.
