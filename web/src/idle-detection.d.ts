interface IdleDetectorOptions {
  threshold: number;
  signal?: AbortSignal;
}

type IdleDetectorPermission = "granted" | "denied";
type IdleUserState = "active" | "idle" | null;
type IdleScreenState = "locked" | "unlocked" | null;

declare class IdleDetector extends EventTarget {
  static requestPermission(): Promise<IdleDetectorPermission>;
  readonly userState: IdleUserState;
  readonly screenState: IdleScreenState;
  start(options: IdleDetectorOptions): Promise<void>;
}