import pawnIoLicense from '../../../src-tauri/crates/mangodisk-privileged/vendors/pawnio/modules/0.2.11/COPYING?raw';

export interface ThirdPartyDocument {
  readonly id: string;
  readonly titleKey: string;
  readonly actionKey: string;
  readonly format: 'text' | 'markdown';
  readonly content: string;
}

export interface ThirdPartyComponent {
  readonly id: string;
  readonly nameKey: string;
  readonly version: string;
  readonly author: string;
  readonly licenseKey: string;
  readonly descriptionKey: string;
  readonly projectUrl: string;
  readonly documents: readonly ThirdPartyDocument[];
}

// Canonical documents are bundled only with the lazy license dialog. Component
// metadata keeps ownership and project links independent of the viewer layout.
export const THIRD_PARTY_COMPONENTS: readonly ThirdPartyComponent[] = [
  {
    id: 'pawnio-modules',
    nameKey: 'thirdPartyLicenses.components.pawnioModules.name',
    version: '0.2.11',
    author: 'namazso',
    licenseKey: 'thirdPartyLicenses.components.pawnioModules.license',
    descriptionKey: 'thirdPartyLicenses.components.pawnioModules.description',
    projectUrl: 'https://github.com/namazso/PawnIO.Modules',
    documents: [
      {
        id: 'license',
        titleKey: 'thirdPartyLicenses.components.pawnioModules.license',
        actionKey: 'thirdPartyLicenses.readLicense',
        format: 'text',
        content: pawnIoLicense,
      },
    ],
  },
  {
    id: 'pawnio-installer',
    nameKey: 'thirdPartyLicenses.components.pawnioInstaller.name',
    version: '2.2.0',
    author: 'namazso',
    licenseKey: 'thirdPartyLicenses.components.pawnioInstaller.license',
    descriptionKey: 'thirdPartyLicenses.components.pawnioInstaller.description',
    projectUrl: 'https://github.com/namazso/PawnIO.Setup',
    documents: [],
  },
];
