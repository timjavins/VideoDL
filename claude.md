# Claude Guidance (Anthropic-style)

## Mission
Build a reliable, inspectable downloader system with conservative error handling and clear user feedback.

## Behavioral rules
- Think in small verifiable steps.
- Prioritize correctness over feature count.
- Explain assumptions explicitly in code comments only when non-obvious.
- Preserve architecture boundaries: UI logic in Python, orchestration in Rust.

## Iteration strategy
1. Extract metadata reliably.
2. Offer explicit quality/subtitle choices.
3. Execute download with progress tracking.
4. Harden error handling and retries.
5. Add GUI and UX polish.

## Safety and compliance posture
- Download only content users are authorized to access.
- Keep platform terms and local laws in mind.
