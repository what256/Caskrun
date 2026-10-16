# Security reporting

Caskrun is an open-source Windows-app launcher in active development. There is no stable binary release or guaranteed security-support window. Reports against the current default branch are welcome; fixes and response times are best effort.

## Report privately

Use GitHub's **Security → Report a vulnerability** for this repository. Keep exploitable details out of public issues until a fix and disclosure plan are agreed. If private reporting is unavailable, open an issue asking the maintainer to enable a private reporting channel without posting the vulnerability details.

Include the affected source revision, a minimal synthetic reproduction, expected and observed behavior, and the impact. Remove personal paths, account details, credentials, real bottle identifiers and unrelated data. Do not attach user bottles, backups, commercial game files or full raw logs. Source snippets and synthetic fixtures are usually sufficient.

## Scope

Useful reports include a Caskrun flaw that lets an untrusted backup escape its destination, accesses unintended host files, stops an unrelated process, exposes private data, or undermines installer or runtime validation. Report flaws in upstream Wine, emulator engines or publisher software to the relevant project's security process, and explain any Caskrun-specific impact separately.

Caskrun launches software the user chooses. Wine bottles are compatibility environments, not a sandbox for untrusted Windows programs. Browser games open in the selected browser and have that browser's security boundaries.

## Sharing diagnostics

Caskrun's structured local diagnostic report uses a limited set of fields. Raw Wine logs, screenshots, launch arguments and bottle backups can contain private data. Review any material before sending it, even in a private report. Repository publication checks help catch accidental disclosures; they do not make arbitrary diagnostics safe to share.
