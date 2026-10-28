# Title details design record

The user accepted the implemented detail screens on 2026-10-18 and authorized merging to main.

Proposal 03 is the approved composition, with its “More episodes” instruction removed. Proposals 01 and 02 are retained as superseded design explorations; their approval gates and validation statements describe their original proposal stage.

The final implementation also adds plain play icons, slightly desaturated/dimmed watched previews, long-press episode actions and VoiceOver equivalents. Automatic status is Watching from 5% through 95% inclusive and Watched above 95%. An explicit manual watched mark takes precedence. Manual marks persist per server/profile/title/season/episode on this iPhone only; they do not fabricate API viewing telemetry. Manual marks can be undone. Explicit rewatch starts from the beginning without deleting history.

Validation: device Debug build and installation on the paired iPhone Air succeeded. The existing app preferences were retained. The user subsequently accepted the finished UI. Automated tests and playback probes were not run, as requested. This record does not claim independently observed playback behavior.

The user also accepted the movie suggestions grid and centered TV section tabs. Movie details show same-type suggestions from the existing cached profile and browse envelopes, prioritizing exact seed shelves and genre matches, with labeled recommendation/explore fallbacks. TV details default to Episodes and provide centered, scroll-pinned Episodes, About and More Like This tabs; season selection persists across tab switches. Suggested titles open inside the detail sheet with back navigation. The final device build and in-place iPhone Air installation succeeded; the user approved merging these additions. No automated tests or playback probes were run.
