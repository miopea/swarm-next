# Security

## Reporting a vulnerability

Report security problems privately: open the repository's **Security** tab and
choose **Report a vulnerability**. Only the maintainer sees the report.

Please do not open a public issue, pull request or discussion about a security
problem until it is fixed.

Include:

- the Swarm version (the Updates card shows it);
- what you did, step by step;
- what an attacker gains.

You will get an answer on the report itself.

## What counts

Anything that lets someone without the operator's credentials:

- run code on a Hive, or reach its worker sessions;
- read or change a Hive's database or settings;
- get a Hive to install a release the maintainer did not sign;
- get past the operator token or a Hive's own sign-in.

## Supported versions

Fixes go into the next release. Only the latest release is supported, and Hives
install ordinary releases by themselves.
