# Security Policy

## Supported versions

Mochi Studio is in early preview. Security fixes are made on the latest release and the `main` branch only.

| Version | Supported |
|---|---|
| 0.1.x (latest) | Yes |
| Older | No |

## Reporting a vulnerability

Please **do not** report security vulnerabilities through public issues, discussions or pull requests.

Instead, use GitHub's private vulnerability reporting: open the repository's **Security** tab and click **Report a vulnerability**. Please include:

- a description of the issue and its impact;
- steps to reproduce, or a proof of concept;
- the Mochi Studio version and your Windows version.

You can expect an acknowledgement within 7 days. Once the issue is confirmed, we will work on a fix and coordinate disclosure with you. We are happy to credit reporters who wish to be named.

## Scope

Mochi Studio runs entirely on your machine and makes no network connections. Issues of particular interest include:

- reading or writing files outside the expected locations;
- crafted media or project files that cause code execution or memory corruption;
- keystroke or screen data being captured when the user has not enabled it.
