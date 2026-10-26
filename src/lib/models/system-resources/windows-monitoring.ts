export type WindowsMonitoringState = 'notInstalled' | 'ready' | 'unavailable' | 'incompatible' | 'unsupportedHardware';
export interface WindowsMonitoringObservation {
  state: WindowsMonitoringState;
  driverAvailable: boolean;
  serviceInstalled: boolean;
}
export type WindowsMonitoringOutcome =
  | 'ready'
  | 'removed'
  | 'unsupportedHardware'
  | 'busy'
  | 'verificationFailed'
  | 'userCancelled'
  | 'installationFailed'
  | 'serviceUnavailable'
  | 'restartRequired'
  | 'driverRemovalFailed'
  | 'removalFailed';
export interface WindowsMonitoringSetupResult {
  outcome: WindowsMonitoringOutcome;
  observation: WindowsMonitoringObservation;
}
