# App Store review preparation

Sources: [Apple’s review guidelines](https://developer.apple.com/app-store/review/guidelines/) and [onboarding guidance](https://developer.apple.com/design/human-interface-guidelines/onboarding), consulted November 1, 2026.

This onboarding update does not establish App Store compliance or guarantee approval.

Before submission:

- **2.1:** Supply a reachable review server, authorized sample media, and connection instructions/QR code.
- **2.3:** Describe the actual client and backend capabilities accurately, including server requirements.
- **4.2:** Demonstrate native browsing, profiles, progress, and playback. A shell alone is insufficient.
- **5.1.1:** Publish an accurate privacy policy, link it in-app and in App Store Connect, and reconcile privacy disclosures with actual data flows.
- **5.2 / 5.2.3:** Establish rights and service authorization for media, metadata, artwork, and streaming sources. Rewording onboarding does not resolve source authorization.

Implemented here: two-screen onboarding, explicit server requirement, optional camera access for QR connection, manual connection fallback, and user-initiated pasting. Content authorization, privacy policy publication, submission metadata, and reviewer infrastructure remain separate submission work. Device testing remains with the developer.
