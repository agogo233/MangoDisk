import { version } from '@tauri-apps/plugin-os';
import type { AppDistribution } from '@/lib/models/updates/app-update';
import type { ClientRequestMetadata } from '@/lib/models/http/client-request';
import { InstallationIdentityService } from '@/lib/services/http/installation-identity-service';

/** Collect at the adapter boundary; callers decide whether missing identity is fatal. */
export class ClientRequestMetadataService {
  static async collect(
    language: string,
    distribution: AppDistribution,
    failure: (source: string, error: unknown) => void
  ): Promise<ClientRequestMetadata> {
    const metadata: ClientRequestMetadata = { locale: language, distribution };
    try {
      metadata.installId = await InstallationIdentityService.getOrCreateInstallId();
    } catch (error) {
      failure('installation_identity', error);
      return metadata;
    }
    try {
      const value = version().trim();
      if (value) metadata.osVersion = value;
    } catch (error) {
      failure('os_version', error);
    }
    try {
      const value = Intl.DateTimeFormat().resolvedOptions().timeZone?.trim();
      if (value) metadata.timezone = value;
    } catch (error) {
      failure('timezone', error);
    }
    return metadata;
  }
}
