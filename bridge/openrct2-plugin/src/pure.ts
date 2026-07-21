// Barrel of pure (no game-global-dependent) functions, bundled separately
// so tests can exercise them without a running OpenRCT2 process.
export { mapWeather, mapRideStatus } from "./observation";
export { randomUuidV7 } from "./uuid";
export { staffTypeToNumber, toGameAction, toErrorInfo } from "./commands";
