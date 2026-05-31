# Codec Conversion UI Spec

## Purpose
Define the GUI behavior for choosing between natural output, editing-friendly converted output, or both.

The goal is to help users make an editing-oriented choice without forcing them to understand codec details up front.

## Product intent
- Default to editing-friendly outputs when a source format is hard to edit.
- Preserve the natural source output when it is already a good editing target.
- Let the user choose one of three output modes: natural, converted, or both.
- Keep the conversion controls visible but progressive, not noisy.

## Terms
- Natural output: the source-adjacent output chosen from the inspected quality.
- Converted output: a second output produced in an editing-friendly target format.
- Friendly source: a source that is already reasonable for editing.
- Unfriendly source: a source that is likely annoying in common editing software or on common export pipelines.

## Source classification
The backend or frontend needs a derived source classification after inspect.

Recommended categories:
- `friendly`
- `unfriendly`
- `unknown`

Recommended signals for the classification:
- video codec name
- audio codec name
- container/extension
- whether the format is already merged or audio-only/video-only

The current inspect payload already gives the GUI enough to know the selected quality, size, and whether audio is present.
It does not yet expose codec names, so the final implementation should add explicit codec metadata instead of guessing from extension alone.

## Default output policy
Use the source classification to choose the initial output mode.

| Source class | Default mode | UI state |
| --- | --- | --- |
| `friendly` | natural | conversion section collapsed |
| `unfriendly` | converted | conversion section expanded |
| `unknown` | natural | conversion section collapsed with a subtle suggestion |

If the user explicitly chooses another mode, preserve that choice.

## Editing-friendly target defaults
These are the recommended target defaults for the conversion panel.

- Compressed video source: `MP4 (H.264/AAC)`
- Lossless video source: `MOV (ProRes)`
- Compressed audio source: `M4A (AAC)`
- Lossless audio source: `WAV`

The GUI should not force the user to memorize these targets.
It should present them as presets with a short reason label such as `best compatibility` or `best editing fidelity`.

## GUI structure
Add a conversion section below the quality selector and above the primary action button.

Suggested layout:
- Source summary row
- Output mode radio group
- Conversion target picker
- Keep-both toggle or inline checkbox
- Small help text / warning text

### Source summary row
Show a compact summary after inspect:
- selected quality label
- source container
- source codec summary when available
- friendly/unfriendly badge

### Output mode selector
Use three mutually exclusive choices:
- Natural only
- Converted only
- Both natural and converted

Rules:
- `Natural only` downloads the source-adjacent output and hides the conversion target picker.
- `Converted only` downloads only the editing-friendly target and shows the conversion target picker.
- `Both` downloads both outputs and shows the conversion target picker.
- These choices apply to merged downloads and split downloads alike.

### Conversion target picker
Show the picker only when `Converted only` or `Both` is selected.

Recommended controls:
- video target dropdown when the selected quality contains video
- audio target dropdown when the selected quality is audio-only
- a compact explanatory line that says why the selected preset was recommended

## Suggested logic branch
When the inspected quality is classified as `unfriendly`, open the conversion section automatically and show a warning banner.

Suggested warning copy:
`This source is not ideal for editing. We recommend a converted copy for smoother timeline work.`

When the inspected quality is classified as `friendly`, keep the conversion section collapsed by default and show a neutral helper note:
`This source is already editing-friendly. Conversion is optional.`

## Interaction flow
1. User enters URL and inspects it.
2. GUI receives source metadata and derived friendliness classification.
3. GUI highlights the selected quality and output suggestion.
4. If source is unfriendly, the conversion section opens and defaults to converted output.
5. User chooses natural, converted, or both.
6. User optionally overrides the conversion preset.
7. GUI sends the download request with the selected output mode and conversion preset.
8. Backend produces the requested output(s) and reports all resulting paths.

## Backend contract required by the GUI
The GUI spec assumes the inspect response can provide these additional fields or equivalent derived values:

- `source_video_codec`
- `source_audio_codec`
- `source_container`
- `source_classification`
- `recommended_output_mode`
- `recommended_conversion_profile`

The download request should accept the same concepts in a machine-readable form:

- output mode: `natural`, `converted`, or `both`
- conversion profile: a named preset such as `mp4_h264_aac`, `mov_prores`, `m4a_aac`, or `wav`

## Error handling
- If the source classification is missing, fall back to `unknown` and default to natural output.
- If a conversion preset is unavailable for the selected source type, disable that preset and explain why.
- If the user selects `Both`, but one branch is impossible, explain the limitation before download starts.
- If ffmpeg or the conversion path is unavailable, keep natural output available and mark conversion as degraded or unavailable.

## Accessibility and clarity
- The output mode control must be keyboard accessible.
- The warning/helper text should be concise and placed near the control it explains.
- Avoid codec jargon in the default copy unless the user expands advanced details.

## Non-goals
- No full codec education panel.
- No manual container/codec editor in the first pass.
- No playlist-level batch conversion rules.
- No automatic destructive replacement of the natural file.

## Acceptance criteria
- After inspect, the GUI can present a clear recommendation for natural, converted, or both.
- Unfriendly source types open the conversion section automatically.
- Users can choose natural, converted, or both before starting the download.
- Users can keep the natural file alongside the converted file when they want both.
- Split downloads can also use converted or both output modes.
- Friendly source types do not force conversion.
- The UI makes it obvious why a conversion was suggested.