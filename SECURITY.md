# Security policy

FeatherRec is experimental. It has no network client, telemetry, account system,
or remote-control interface. Screen recordings and diagnostic reports remain local.
The user explicitly starts monitor capture. Protected content may be blank.

Report a vulnerability using GitHub's private vulnerability reporting feature if
available. Otherwise open an issue containing a minimal description and ask for a
private channel before sharing exploit details. Never include credentials, private
screen recordings, or personal data in a public issue.

Only the current development branch is receiving fixes. No production support or
security audit is claimed. Forced termination, power loss and full disks can leave
an incomplete `.partial.mp4`; FeatherRec retains it instead of calling it successful.

Settings and reports are plaintext local files. They may contain output paths and
hardware encoder names. Review them before sharing. The program does not upload them.
