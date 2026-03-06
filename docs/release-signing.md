# Release Signing Workflow

## What is in the repo now

- `scripts\release-msix.ps1` builds the viewer MSIX, updates the package publisher/version for the release build, bundles the helper release binary when available, and optionally signs the final `.msix`.
- `.github\workflows\release.yml` packages the release artifact on tags or manual dispatch, uploads the MSIX, and generates winget manifests from the same package.
- `scripts\build-viewer.cmd --msix` now routes through the same release packaging path.

## Required repository configuration

Configure these values in GitHub before enabling signed releases:

- Secret `MSIX_CERT_BASE64`: base64-encoded PFX contents
- Secret `MSIX_CERT_PASSWORD`: PFX password
- Secret `MSIX_TIMESTAMP_URL`: optional RFC3161 timestamp URL
- Variable `MSIX_PUBLISHER`: certificate subject that must exactly match the MSIX manifest publisher, for example `CN=Contoso Software LLC`

If the signing secrets are absent, the workflow still produces an unsigned MSIX and an unsigned winget manifest set for rehearsal purposes.

## Current stop point

- The repository is intentionally stopping short of a localhost `winget install/upgrade/uninstall` rehearsal on this machine.
- That final rehearsal still requires a signed MSIX plus a trusted certificate chain and would modify local trust and package-install state.
- Until those inputs are available on a suitable test machine, keep `R-07` in progress and use the current workflow only for package and manifest generation/validation.
- When a disposable test machine is ready, execute `docs\winget-rehearsal-runbook.md` and record the outcome in `docs\qa-matrix.md`.

## Manual release flow

1. Create or choose the target package version, for example `1.0.0.0`.
2. Run:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\release-msix.ps1 -BundleHelperRelease -RuntimeIdentifier win-x64 -Version 1.0.0.0 -Publisher "CN=SingularityMonitor"
```

3. To sign locally, add:

```powershell
-Sign -CertificateBase64 $env:MSIX_CERT_BASE64 -CertificatePassword $env:MSIX_CERT_PASSWORD -TimestampUrl $env:MSIX_TIMESTAMP_URL
```

4. Verify the final package:

```powershell
signtool verify /pa /v "viewer\AppPackages\Release\<package>.msix"
Get-AuthenticodeSignature "viewer\AppPackages\Release\<package>.msix" | Format-List Status, SignerCertificate, TimeStamperCertificate
```

## GitHub Actions flow

- Tag-driven release: push a tag like `v1.0.0.0`
- Manual release: run the `Release` workflow and supply a version and runtime identifier

The workflow:

1. Builds the helper release binary
2. Packages the viewer MSIX
3. Signs when secrets are present
4. Uploads the MSIX artifact
5. Publishes the tag asset to GitHub Releases
6. Generates and validates winget manifests from the same package

## Important constraints

- The signing certificate subject must match the manifest `Publisher` exactly.
- Changing the publisher changes package identity and breaks seamless upgrades.
- The current packaged release flow bundles `helper.exe`, but the daemon service is still installed separately.
- Unsigned packages are only for CI rehearsal and local testing.
