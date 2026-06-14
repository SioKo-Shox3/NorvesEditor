#ifndef NORVES_BRIDGE_DTO_COMMON_HPP
#define NORVES_BRIDGE_DTO_COMMON_HPP

#include <optional>
#include <string_view>

// Shared wire enums for the typed payload DTOs (F5).
//
// Depends on <std> only; no third-party headers are exposed here. These mirror
// the enum $defs of bridge/spec/schema/common.schema.json (and the Rust
// reference bridge/crates/norves-bridge-core/src/common.rs) one-for-one. The
// wire strings below are the canonical contract; changing either an enum member
// or its wire string is a protocol change.
//
// Only the enums exercised by the F5 round-trip (engineState, runtimeState,
// logLevel) are modelled. viewportState / origin and the open value $defs
// (propertyValue, sceneNode, ...) are intentionally NOT typed here; they stay
// opaque (carried as JsonValue) until a later phase needs them.
namespace norves::bridge::dto {

// common.schema.json#/$defs/engineState: ["initializing","ready","running","error"].
enum class EngineState { Initializing, Ready, Running, Error };

// common.schema.json#/$defs/runtimeState: ["edit","playing","paused","stopped","unknown"].
enum class RuntimeState { Edit, Playing, Paused, Stopped, Unknown };

// common.schema.json#/$defs/logLevel: ["trace","debug","info","warn","error"].
enum class LogLevel { Trace, Debug, Info, Warn, Error };

// Wire-string conversions. to_wire is total (every enumerator has a string);
// from_wire returns nullopt for an unknown member, mirroring serde's rejection
// of an out-of-enum value.
[[nodiscard]] std::string_view to_wire(EngineState value);
[[nodiscard]] std::string_view to_wire(RuntimeState value);
[[nodiscard]] std::string_view to_wire(LogLevel value);

[[nodiscard]] std::optional<EngineState> engine_state_from_wire(std::string_view text);
[[nodiscard]] std::optional<RuntimeState> runtime_state_from_wire(std::string_view text);
[[nodiscard]] std::optional<LogLevel> log_level_from_wire(std::string_view text);

}  // namespace norves::bridge::dto

#endif  // NORVES_BRIDGE_DTO_COMMON_HPP
