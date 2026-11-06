# `.fig` (fig-kiwi) field names

What the importer reads from the kiwi `NodeChange` messages, and whether each
name has been seen in a real file. Only field names and value shapes are
recorded here; nothing from the files' content.

Real files checked: a library-heavy product UI kit file (`private-ui-kit.fig`,
about 11k node changes, 7 libraries, 112 variables) and the shadcn/ui community
file (`shadcn-ui.fig`, about 5.4k node changes, everything local). Both are
private local copies and are never committed. "Confirmed" means seen in at
least one of them. "Unconfirmed" means the name is a guess (kept because it is
harmless when absent) or was never present in these files.

A third file, a game UI file (`game-ui.fig`, about 47k node changes, container
version 106, private as well), was checked node by node against the REST API's
JSON for the same file; rows that say "the third file" refer to it.

Dump any file with `cargo run --release -p desigma-fig --example fig2json -- file.fig`
and render its frames with `--example fig_render`.

## Structure

| Feature | Fields | Status |
|---|---|---|
| Tree | `guid {sessionID, localID}`, `parentIndex {guid, position}` (fractional index string), `type`, `phase` | confirmed |
| Pages | `CANVAS` nodes under `DOCUMENT`; `backgroundColor`, `internalOnly` | confirmed |
| Internal canvas | `internalOnly: true` canvas holds the remote SYMBOLs, state-group FRAMEs, style nodes, VARIABLE and VARIABLE_SET nodes the file uses (many thousands of nodes) | confirmed |
| Node types | DOCUMENT CANVAS FRAME SYMBOL INSTANCE TEXT VECTOR ELLIPSE ROUNDED_RECTANGLE LINE STAR BOOLEAN_OPERATION SECTION VARIABLE VARIABLE_SET | confirmed. `RECTANGLE`, `REGULAR_POLYGON`, `GROUP` were not present (kept) |
| Placement | `transform {m00..m12}`, `size {x, y}`, `opacity`, `visible`, `locked`, `blendMode`, `mask` | confirmed except `mask` (unconfirmed) |
| Constraints | `horizontalConstraint` / `verticalConstraint`: `MAX`, `CENTER`, `STRETCH`, `SCALE`; absent = min | confirmed |
| Corners | `cornerRadius`, `rectangle{TopLeft,TopRight,BottomLeft,BottomRight}CornerRadius`, `rectangleCornerRadiiIndependent`, `cornerSmoothing` | confirmed |
| Strokes | `strokePaints`, `strokeWeight`, `strokeAlign` (INSIDE/OUTSIDE/CENTER), `strokeCap` (only ROUND seen), `strokeJoin`, `dashPattern`, `border{Top,Right,Bottom,Left}Weight`, `borderStrokeWeightsIndependent`, `miterLimit` | confirmed |
| Frames | `frameMaskDisabled` (absent = clips) | confirmed |
| Groups | no `GROUP` node type: a group is a `FRAME` with `resizeToFit: true` and no `stackMode`; imported as `GROUP` (all 274 checked against REST ids in a third file) | confirmed |
| Auto layout | `stackMode` HORIZONTAL/VERTICAL, `stackSpacing`, `stackCounterSpacing`, `stackHorizontalPadding`, `stackVerticalPadding`, `stackPaddingRight`, `stackPaddingBottom`, `stackPrimaryAlignItems` (CENTER/MAX/SPACE_EVENLY), `stackCounterAlignItems`, `stackChildAlignSelf` (STRETCH/MAX/CENTER), `stackChildPrimaryGrow`, `stackReverseZIndex` | confirmed |
| Auto layout sizing | `stackPrimarySizing`: only `FIXED` is ever written, absent means hug. `stackCounterSizing`: only `RESIZE_TO_FIT_WITH_IMPLICIT_SIZE` is written, absent means fixed. (Checked against stored sizes: absent primary hugs its children.) Fixed in this change; it used to read absent primary as fixed | confirmed |
| Wrap / absolute / min-max | `stackWrap`, `stackPositioning`, `minSize` / `maxSize` | unconfirmed |
| Star | `count`, `starInnerScale` | confirmed |
| Arc | `arcData {startingAngle, endingAngle, innerRadius}` | confirmed |
| Boolean ops | `booleanOperation`: UNION, SUBTRACT, INTERSECT, XOR (mapped to EXCLUDE) | confirmed |
| Layout grids | `layoutGrids` (`pattern`/`type`/`axis`, `sectionSize`, `gutterSize`, `count`, `offset`) | **unconfirmed** (no node in either file has the field) |
| Guides | `guides`: present as an empty list on symbols and instances; element shape (`axis`, `offset`) | field confirmed, element shape unconfirmed |
| Export settings | `exportSettings [{imageType PNG/SVG, constraint {type CONTENT_SCALE, value}, suffix, contentsOnly, colorProfile, svg*}]` | confirmed (JPEG/PDF, `CONTENT_WIDTH/HEIGHT` unconfirmed) |
| FigJam sections | In a FigJam file `SECTION` maps to a board section: `fills` (nearest of the 16 section colours), `sectionContentsHidden`, children. In a design file it stays a design section | `SECTION` confirmed, `sectionContentsHidden` unconfirmed (no FigJam file at hand) |
| FigJam tables | `TABLE` (`numRows`, `numColumns`) with `TABLE_CELL` children (`rowIndex`, `columnIndex`, `characters`, `fills`, `size`). Column widths come from the first row's cells, row heights from the first column's; missing cells are padded, extras dropped | **unconfirmed**: the field names follow the plugin API's table node; no FigJam file with a table was available to check them |
| Dev status / annotations | `sectionStatusInfo {status, prevStatus, lastUpdateUnixTimestamp}` exists on symbols; not mapped. No `devStatus` or annotation fields appear | unconfirmed |
| Plugin / edit metadata | `pluginData [{pluginID, key, value}]`, `editInfo {createdAt, lastEditedAt, userId}` | confirmed, ignored |

## Paints, effects, geometry

| Feature | Fields | Status |
|---|---|---|
| Paints | `fillPaints` entries: `type` SOLID / GRADIENT_LINEAR / GRADIENT_RADIAL / IMAGE, `color {r,g,b,a}`, `opacity`, `visible`, `blendMode`, `authoredColor`, `colorVar` | confirmed. ANGULAR / DIAMOND gradients unconfirmed |
| Gradient | `stops [{color, position}]` (`stopsVar` adds `colorVar`), `transform {m00..m12}` | confirmed |
| Image | `image {hash, name}`, `imageScaleMode` FILL / STRETCH; hashes match `images/` entries of the zip | confirmed (FIT / TILE unconfirmed) |
| Effects | `effects [{type, color, offset, radius, spread, showShadowBehindNode, blendMode, visible, colorVar}]`: DROP_SHADOW, INNER_SHADOW | confirmed. LAYER_BLUR / BACKGROUND_BLUR (`FOREGROUND_BLUR`) and NOISE / TEXTURE / GLASS names and their parameters are **unconfirmed** |
| Geometry | `fillGeometry` / `strokeGeometry [{commandsBlob, styleID, windingRule}]`, blobs index `message.blobs` | confirmed (`NONZERO` only) |
| Vectors | `vectorData {vectorNetworkBlob, normalizedSize, styleOverrideTable [{styleID, ...}]}` | confirmed |
| Text outlines | `derivedTextData {baselines, glyphs [{commandsBlob, position, fontSize}], fontMetaData, layoutSize, ...}` (unused: the renderer lays text out itself) | confirmed |

## Text

| Feature | Fields | Status |
|---|---|---|
| Content | `textData {characters, lines [{lineType, indentationLevel, isFirstLineOfList, listStartOffset, sourceDirectionality, styleId}]}` | confirmed |
| Line types | `lineType` `PLAIN`, `UNORDERED_LIST` (`ORDERED_LIST` unconfirmed) | partly confirmed |
| Runs | `textData.characterStyleIDs`, `textData.styleOverrideTable [{styleID, fontName, fontSize, styleIdForFill, fontVariant*}]` | confirmed |
| Font | `fontName {family, style, postscript}`, `fontSize`, `fontVersion`, `fontVariations` | confirmed |
| Linked text style | `styleIdForText` resolves through the shared style node before character overrides. Node-level font values can retain stale pre-link values (48px while the linked style and derived glyphs use 18px). Preserve node paints and alignment; resolve percentage run tracking against the linked font size. | confirmed |
| Line height | `lineHeight {value, units}`: `PIXELS` (almost all) and `PERCENT`; `RAW` unconfirmed | confirmed |
| Letter spacing | `letterSpacing {value, units}`: always `PERCENT` here; `textTracking` is a redundant float fraction | confirmed |
| Alignment | `textAlignHorizontal` (absent = left), `textAlignVertical`, `textAutoResize` WIDTH_AND_HEIGHT / HEIGHT (`NONE`, `TRUNCATE` unconfirmed) | confirmed |
| Decoration | `textDecoration` UNDERLINE; `textCase` not present | partly confirmed |
| OpenType | `toggledOffOTFeatures ["LIGA"]` (and the guessed `toggledOnOTFeatures`); `fontVariantCommonLigatures` / `fontVariantContextualLigatures` are booleans, `fontVariantNumericFigure` / `Spacing` / `Fraction` / `Caps` are enums with value `NORMAL` | confirmed. Non-default enum values unconfirmed |
| Other | `leadingTrim` `CAP_HEIGHT`, `paragraphSpacing`, `hyperlink {url}`, `emojiImageSet`, `textBidiVersion`, `autoRename`, `textTruncation` / `maxLines` | `leadingTrim`, `paragraphSpacing`, `hyperlink` confirmed; truncation unconfirmed |

## Components and instances

| Feature | Fields | Status |
|---|---|---|
| Component | `SYMBOL` node; `componentKey`, `sourceLibraryKey` (`lk-...`, one per library), `overrideKey`, `publishID`, `isSymbolPublishable`, `symbolDescription` | confirmed |
| Component set | `FRAME` with `isStateGroup: true`, `stateGroupPropertyValueOrders [{property, values}]`; variants carry `variantPropSpecs [{propDefId, value}]` | confirmed |
| Instance | `symbolData {symbolID, symbolOverrides [...], uniformScaleFactor}`, `derivedSymbolData [...]`, `derivedSymbolDataLayoutVersion`, `propsAreBubbled`. **Instances never store children** | confirmed |
| Override paths | `guidPath {guids [..]}` holds `overrideKey` values, not node guids (1 to 3 entries; nested instances take two or more) | confirmed |
| Instance copy ids | REST names a copied node `I<instance>;<a>;<b>`: one `I`, then the flat path. A segment is the node's `overrideKey` when it has one (the library-side id of a library component copy), else its guid. Matches REST ids for all 10,127 instance-internal nodes of the third file | confirmed |
| Override content | any NodeChange fields: `size`, `transform`, `fillPaints`, `strokePaints`, `strokeWeight`, `textData`, `fontName`, `styleIdForFill` (`{assetRef}`), `fillGeometry`, `strokeGeometry`, ... | confirmed |
| Nested instance swap | override entry `{guidPath: [<slot>], overriddenSymbolID: <SYMBOL guid>}` replaces the nested instance's component; deeper entries under `[<slot>, ...]` then address the swapped component's children. A slot with `uniformScaleFactor` keeps it: the swapped-in contents are scaled too (the third file has such slots at 0.875 and 2) | confirmed (product UI file: list rows swapping their icon slot) |
| Override paints bound to variables | `fillPaints` / `strokePaints` inside an override entry carry `colorVar` like node paints do; the literal `color` beside the alias can be stale (a dark-theme text override stored `#0A0A0A` next to an alias whose value is near-white), so the variable's value wins | confirmed (product UI file: dark-theme instances recolouring component text) |
| Derived data | per path: `size`, `transform`, `fillGeometry`, `strokeGeometry` | confirmed |
| Scale | `uniformScaleFactor` != 1 on instances made with the scale tool: copied contents scale by it | confirmed |
| Property defs | `componentPropDefs [{id, name, type BOOL/TEXT/VARIANT/INSTANCE_SWAP, initialValue {boolValue / textValue{characters} / guidValue}, varValue, preferredValues, sortPosition}]` on the set; variants carry bare `{id, parentPropDefId}` aliases. In a third file nearly every def had only `varValue {dataType, resolvedDataType, value {boolValue / textDataValue{characters} / symbolIdValue{guid}}}` and no `initialValue` | confirmed |
| Property values | instance `componentPropAssignments [{defID, value {boolValue / textValue / guidValue}, varValue}]`; `value` may be absent, leaving only `varValue` (same shape as in defs): 1,602 of 1,710 instance swaps and 86 of 185 booleans in the third file | confirmed |
| Property refs | node `componentPropRefs [{defID, componentPropNodeField VISIBLE / TEXT_DATA / OVERRIDDEN_SYMBOL_ID}]`, with `parameterConsumptionMap` repeating them. The third file has no `componentPropRefs` at all: only `parameterConsumptionMap {entries [{variableField VISIBLE / TEXT_DATA / OVERRIDDEN_SYMBOL_ID, variableData {dataType PROP_REF, value {propRefValue {defId}}}}]}`. All defIDs go through `parentPropDefId` | confirmed |
| Detached | `detachedSymbolId {assetRef}` | confirmed, ignored |

## Styles and libraries

| Feature | Fields | Status |
|---|---|---|
| Style nodes | any node with `styleType` FILL / TEXT / EFFECT (GRID unconfirmed), `key` (library key, absent in local files), `version`, `styleDescription`, `sortPosition`, `isSoftDeleted` | confirmed. The guessed `sharedStyleMasterData.styleKey` does not exist; `key` replaces it |
| Style refs | `styleIdForFill`, `styleIdForStrokeFill`, `styleIdForText`, `styleIdForEffect`: `{assetRef: {key, version}}` for library styles, `{guid}` for styles of the same file (`styleIdForGrid` unconfirmed) | confirmed |
| Libraries | `{assetRef: {key, version}}` is how every library asset is referenced; `key` equals the target node's `key` | confirmed |

## Variables

| Feature | Fields | Status |
|---|---|---|
| Sets | `VARIABLE_SET`: `name`, `key`, `variableSetModes [{id, name, sortPosition}]` | confirmed |
| Variables | `VARIABLE`: `name`, `key`, `variableSetID` (`{assetRef: {key}}`, or `{guid}` when local), `variableResolvedType` COLOR / FLOAT / STRING (BOOLEAN unconfirmed), `variableScopes`, `variableDataValues {entries [{modeID, variableData {dataType, resolvedDataType, value}}]}`, `sortPosition` | confirmed |
| Values | `value`: `colorValue {r,g,b,a}`, `floatValue`, `textValue` (string), `alias {assetRef {key}}` (a local `{guid}` alias is unconfirmed) | confirmed |
| Bindings | node `variableConsumptionMap {entries [{variableField, variableData}]}` with fields such as `STACK_PADDING_*`, `STACK_SPACING`, `RECTANGLE_*_CORNER_RADIUS`, `CORNER_RADIUS`, `WIDTH`, `HEIGHT`, `FONT_SIZE`, `FONT_FAMILY`, `FONT_STYLE`; paint, stop and effect colors use `colorVar` | confirmed |
| Modes | node `variableModeBySetMap {entries [{variableModeID, variableSetID}]}` | confirmed (empty in most nodes) |

## Prototypes

`prototypeInteractions [{id, isDeleted, event {interactionType ON_CLICK / ON_HOVER / MOUSE_IN / MOUSE_OUT}, actions [{connectionType INTERNAL_NODE, navigationType SWAP_STATE, transitionNodeID, transitionType DISSOLVE, transitionDuration, easingType, easingFunction}]}]`: confirmed. URL / overlay / scroll fields (`connectionURL`, `overlay*`, `overflowDirection`, `flowStartingPoint`) are unconfirmed.


## Motion (Figma Motion, Beta)

Decoded from a small Motion file (one rectangle, Position / Rotation / Opacity keyframes at 0 and 0.997 s, one segment set to a spring, the "Burst" preset applied at 997 ms). "Confirmed" = present in that file and verified against what the UI showed; "schema" = the field exists in the file's kiwi schema but the sample has no data for it; "guess" = our interpretation of an unverified value. Implemented in `src/motion.rs`; synthetic coverage in `tests/motion_import.rs`, the private file in `tests/real_files.rs` (ignored).

Where things live:

| What | Fields | Status |
|---|---|---|
| Animated node | the ordinary node (here `ROUNDED_RECTANGLE`) with `parameterConsumptionMap {entries [{variableField, variableData}]}` (same shape as `variableConsumptionMap`, but for animation) and `animationPresets {presets [{animationPresetId {guid}, sortPosition}]}` | confirmed |
| Keyframed field | `variableData.dataType EXPRESSION`, `value.expressionValue {expressionFunction KEYFRAME, expressionArguments [base value, {dataType KEYFRAME_TRACK_PARAMETER_DATA, value.keyframeTrackParameterValue {parameters [...]}}]}`. The base argument is the static value (`floatValue`/`vectorValue`) | confirmed |
| Track reference | parameter `{type MANUAL, value.manual.keyframeTrackId.guid}` (a track keyed by hand) or `{type ANIMATION_PRESET, value.animationPreset {animationPresetId.guid, keyframeTrackId.guid}}` (a track owned by a preset instance). A field can list both: manual tracks and preset tracks are separate | confirmed |
| Track | node `KEYFRAME_TRACK` (name "Keyframe Track"), parented to the internal-only canvas (manual) or to its `ANIMATION_PRESET_INSTANCE` (preset). Preset tracks carry `keyframeOperation` (`SET`, `SCALE` = multiply, `OFFSET` = add; manual tracks omit it = SET). Schema also has `trackLoopMode` NONE / LOOP | confirmed (`keyframeOperation`); schema (`trackLoopMode`) |
| Keyframe | node `KEYFRAME` (name "Keyframe"), parent = its track. `timelinePosition` (Int64, **microseconds**; absent on the first keyframe = 0), `keyframeValue {value {floatValue \| vectorValue {x,y}}, valueType FLOAT / VECTOR}` (schema also: COLOR, TEXT_DATA, BOOL, CIRCLE, LINE, PAINT, ROTATION_3D...), `easingData`, vector tracks also `motionPathBezierHandles {p1x,p1y,p2x,p2y}` (all 0 in the sample; schema type `BezierHandles`). Schema: `timelinePositionType` ABSOLUTE / RELATIVE (sample: absent = absolute) | confirmed |
| Preset instance | node `ANIMATION_PRESET_INSTANCE`, name `motion.preset_name.<position\|scale\|rotation\|opacity>` (one per basic preset), parent = internal canvas. `timelineOffset` (Int64 µs, here 997000 = where it was applied), `backingCodeComponentId {assetRef {key, version}}`, `componentPropAssignments [{defID, value, varValue}]` | confirmed |
| Preset definition | node `CODE_COMPONENT`, `codeObjectType ANIMATION_PRESET`, `key` = the instance's `backingCodeComponentId.assetRef.key`, `componentPropDefs [{id, name, type, varValue}]`. `defID` of an assignment = `id` of a def; the **name** (`type`, `direction`, `distance`, `amount`, `axis`, `start`/`end`, `startX`..., `delay`, `duration`, `easing`) says what it is | confirmed |
| Applying "Burst" | there is **no** "Burst" name in the file: applying an Enter/Cycle/Exit preset just creates one instance per basic preset it consists of (Burst = position + scale + rotation + opacity) with those presets' parameters. The 41 named presets therefore cannot be recognised by name; clips are rebuilt from the parameters | confirmed |

Units and value encoding:

| Item | Encoding | Status |
|---|---|---|
| Time | keyframe `timelinePosition` and `timelineOffset`: Int64 **microseconds**. Preset parameters `delay`, `duration`: float **seconds** (`resolvedDataType TIMING`), e.g. `0.997`, `0.3` | confirmed |
| Fields (`variableField`) | `MOTION_TRANSLATION_XY` (vector, **offset** from the node's static position, y down), `MOTION_TRANSLATION_X` / `_Y` (float offsets, used by preset tracks), `MOTION_ROTATION` (float, **radians, offset**, sign opposite to the Rotation field: UI 90 degrees is stored `-1.5707964`), `MOTION_SCALE_X` / `_Y` (float factors, 1 = unchanged), `OPACITY` (float **percent**, 100 = opaque, absolute; preset opacity tracks are 0..1 factors with `keyframeOperation SCALE`) | confirmed (these six); the UI showed Position -7 -> 200 for offsets 0 -> 207 |
| Other fields | schema also has `MOTION_TRANSLATION_XY`-style `MOTION_SCALE_XY`, `MOTION_SHEAR`, `MOTION_TRANSFORM_ORIGIN_*`, `WIDTH`, `HEIGHT`, `CORNER_RADIUS`, `STROKE_WEIGHT`, `X_POSITION`, `Y_POSITION`, `ROTATION`, `PATH_TRIM_*`, `TRANSFORM_3D_*`, `TIMELINE_DURATION`, `TEXT_ELEMENT_DELAY`, plus `keyframeBinding*` for paint/effect fields | schema only (we map `MOTION_SCALE_XY`, `WIDTH`, `HEIGHT`, `CORNER_RADIUS`, `STROKE_WEIGHT` as absolute values: guess) |
| Easing | `easingData {easingType, easingValue {bezierEasing {p1x,p1y,p2x,p2y}, springEasing {stiffness, damping, mass}}}`. `easingType`: `LINEAR`, `HOLD`, `IN_CUBIC`, `OUT_CUBIC`, `INOUT_CUBIC`, `IN_BACK_CUBIC`, `OUT_BACK_CUBIC`, `INOUT_BACK_CUBIC`, `CUSTOM_CUBIC` (uses `bezierEasing`), `SPRING`, `GENTLE_SPRING`, `CUSTOM_SPRING` (uses `springEasing`; sample 100 / 15 / 1; a `bezierEasing` of 0,0,1,1 is also written), `SPRING_PRESET_ONE/TWO/THREE`, `EASE_IN` | confirmed: `LINEAR`, `CUSTOM_CUBIC`, `CUSTOM_SPRING`, `OUT_CUBIC` (the preset default). Other names are from the schema; their mapping is a guess |
| Default easing | new keyframes are `CUSTOM_CUBIC` 0.5, 0, 0.5, 1 (the "Ease in and out" shown in the UI); the first position keyframe is `LINEAR` | confirmed |
| Which key owns a segment's easing | the **end** keyframe of the segment (the sample's spring sits on the second Position key, which is the segment the UI changed). desigma stores it on the segment's start key, so it is shifted back by one | confirmed by the spring; the rest follows from it |
| Preset parameters seen | position: `type` `slide_in` (also `slide_out`, `custom`), `direction` `bottom`, `distance` 116; scale: `type` `scaleIn`, `amount` 80 (percent); rotation: `type` `rotateIn`, `direction` `clockwise`, `amount` 90 (degrees); opacity: `type` `fadeIn`; all: `delay`, `duration` (s), `easing` (`componentPropAssignment.value.easingData`, mirrored in `varValue.value.easingValue`). Unassigned parameters take the def's default | confirmed |
| Timeline | **nothing is stored for the defaults** (2000 ms, no loop, as in the sample). Schema: node `timelineDuration` (Uint64 µs), `timelineDefinitions {entries [{id, data {durationUs, defaultTimeline, playbackStyle ONCE / LOOP / BOOMERANG, autoplay, keyframeSnappingFps {numerator, denominator}, quantizePlayback}}]}`, `timelineAssignments`, `timelineDisabled`, `playbackStyle`. We read a page's `timelineDuration` / default `timelineDefinitions` entry | schema only |

How it maps (`src/motion.rs`):

* Manual tracks become `Track`s of the matching `AnimProperty`. Position offsets are added to the node's static position, rotation (negated, to degrees) to its static rotation, so desigma stores absolute values like its own editor. X / Y channels are merged on the union of their key times. Opacity percent becomes 0..1.
* Each key's easing is taken from the segment's end keyframe (see above). Springs become `CustomSpring { bounce }` with `zeta = damping / (2 sqrt(stiffness mass))`, `bounce = 1 - zeta` (or `1 / zeta - 1` when over-damped); the sample's 100 / 15 / 1 is exactly desigma's default bounce 0.25. `CUSTOM_CUBIC` keeps its control points (`Custom bezier`), named cubics map to the matching preset curve.
* Each preset instance becomes one `Clip` (kind from the fields its tracks drive, mode / direction / amount from the parameters, cross-checked against the track values; a mismatch becomes a `Custom` clip with the exact from / to). `timelineOffset` is the delay, `duration` and `easing` come from the parameters.
* Guesses (no sample): looping tracks make `ClipRepeat::Infinite`, `playbackStyle BOOMERANG` on an instance makes `yoyo`.
* `motionPathBezierHandles` (the curved motion path of a Position track) has no desigma equivalent and is dropped; `MOTION_SHEAR`, transform origin, 3D, path trim, colour / paint / effect tracks and anything unknown are skipped without failing the import.
* The track, keyframe, preset-instance and preset-definition nodes are not imported as nodes.

Open question: the file's node transform is x = -101, y = -199 while the UI showed x = -7 / y = -105 at time 0 (a constant 94 = half the 188 px size); we import the static transform as is, so the keyframed position is `transform + offset`.

Export: desigma has no kiwi writer (only REST-shaped JSON), so a `.fig` cannot be written. `export_rest_json` writes `motion` (the serde form of `NodeMotion`) on animated nodes and `motionTimeline` on pages with non-default timeline settings, which `import_rest_json` reads back losslessly. Figma's REST API has no motion fields, so these keys are desigma extensions.
