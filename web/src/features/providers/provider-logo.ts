import codexLogo from '../../../../assets/providers/codex.webp';
import commandCodeLogo from '../../../../assets/providers/commandcode.webp';
import kiloCodeLogo from '../../../../assets/providers/kilocode.webp';
import openCodeLogo from '../../../../assets/providers/opencode.webp';
import openRouterLogo from '../../../../assets/providers/openrouter.webp';
import nvidiaLogo from '../../../../assets/providers/nvidia.webp';
import clineLogo from '../../../../assets/providers/cline.webp';
import freebuffLogo from '../../../../assets/providers/freebuff.webp';
import antigravityLogo from '../../../../assets/providers/antigravity.webp';

const adapterLogos: Record<string, string> = {
  command_code: commandCodeLogo,
  kilo_gateway: kiloCodeLogo,
  openai_codex: codexLogo,
  opencode_go: openCodeLogo,
  opencode_zen: openCodeLogo,
  openrouter: openRouterLogo,
  nvidia_nim: nvidiaLogo,
  cline: clineLogo,
  clinepass: clineLogo,
  freebuff: freebuffLogo,
  antigravity: antigravityLogo,
};

export function providerLogoSrc(value?: string | null): string | null {
  if (!value || value.length > 2048) return null;
  try {
    const url = new URL(value);
    const host = url.hostname.toLowerCase().replace(/^\[|\]$/g, '');
    const ipv4 = host.split('.').map(Number);
    const isIpv4 = ipv4.length === 4 && ipv4.every((part) => Number.isInteger(part) && part >= 0 && part <= 255);
    const privateIpv4 = isIpv4 && (
      ipv4[0] === 0 || ipv4[0] === 10 || ipv4[0] === 127 || ipv4[0] >= 224
      || (ipv4[0] === 169 && ipv4[1] === 254)
      || (ipv4[0] === 172 && ipv4[1] >= 16 && ipv4[1] <= 31)
      || (ipv4[0] === 192 && ipv4[1] === 168)
      || (ipv4[0] === 100 && ipv4[1] >= 64 && ipv4[1] <= 127)
    );
    const privateIpv6 = host.includes(':') && (
      host === '::' || host === '::1' || host.startsWith('fc') || host.startsWith('fd')
      || host.startsWith('fe80:') || host.startsWith('::ffff:')
    );
    const localHostname = host === 'localhost' || host.endsWith('.localhost')
      || host === 'local' || host.endsWith('.local')
      || host.endsWith('.localdomain') || host.endsWith('.lan')
      || host === 'internal' || host.endsWith('.internal')
      || host === 'home' || host.endsWith('.home') || host.endsWith('.home.arpa')
      || !host.includes('.') || ['.test', '.invalid', '.example', '.onion'].some((suffix) => host.endsWith(suffix));
    if (url.protocol !== 'https:' || url.username || url.password || localHostname || privateIpv4 || privateIpv6) return null;
    return url.href;
  } catch {
    return null;
  }
}

export function providerDefaultLogo(
  adapterId?: string | null,
  providerId?: string | null,
  name?: string | null,
): string | null {
  const adapterLogo = adapterId ? adapterLogos[adapterId] : undefined;
  if (adapterLogo) return adapterLogo;

  const identities = [providerId, name]
    .filter((value): value is string => Boolean(value))
    .map((value) => value.toLowerCase().replace(/[^a-z0-9]/g, ''));
  if (identities.some((value) => value.includes('opencode'))) return openCodeLogo;
  if (identities.some((value) => value === 'codex' || value.endsWith('codex'))) return codexLogo;
  if (identities.some((value) => value.includes('commandcode'))) return commandCodeLogo;
  if (identities.some((value) => value.includes('kilocode') || value.includes('kiloaigateway'))) return kiloCodeLogo;
  if (identities.some((value) => value.includes('cline'))) return clineLogo;
  if (identities.some((value) => value.includes('antigravity'))) return antigravityLogo;
  return null;
}

export function providerLogo(
  value?: string | null,
  adapterId?: string | null,
  providerId?: string | null,
  name?: string | null,
): string | null {
  return providerLogoSrc(value) ?? providerDefaultLogo(adapterId, providerId, name);
}
