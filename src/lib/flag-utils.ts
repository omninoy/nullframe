/**
 * Get flag icon CSS class for a country code (ISO 3166-1 alpha-2)
 */
export function getFlagIconClass(countryCode: string): string {
  if (countryCode?.length !== 2) {
    return "";
  }
  return `fi fi-${countryCode.toLowerCase()}`;
}

const COUNTRY_NAME_TO_CODE: Record<string, string> = {
  singapore: "SG",
  netherlands: "NL",
  "the netherlands": "NL",
  "united states": "US",
  "united kingdom": "GB",
  germany: "DE",
  france: "FR",
  russia: "RU",
  "hong kong": "HK",
  japan: "JP",
  canada: "CA",
  australia: "AU",
  finland: "FI",
  sweden: "SE",
  poland: "PL",
  ukraine: "UA",
  kazakhstan: "KZ",
  turkey: "TR",
  switzerland: "CH",
  austria: "AT",
  spain: "ES",
  italy: "IT",
  latvia: "LV",
  estonia: "EE",
  lithuania: "LT",
  "czech republic": "CZ",
  czechia: "CZ",
  moldova: "MD",
  romania: "RO",
  bulgaria: "BG",
  israel: "IL",
  "united arab emirates": "AE",
  brazil: "BR",
  argentina: "AR",
  india: "IN",
  "south korea": "KR",
  taiwan: "TW",
  thailand: "TH",
  vietnam: "VN",
  indonesia: "ID",
  malaysia: "MY",
  norway: "NO",
  denmark: "DK",
  belgium: "BE",
  ireland: "IE",
  portugal: "PT",
  greece: "GR",
  hungary: "HU",
  slovakia: "SK",
  croatia: "HR",
  serbia: "RS",
  slovenia: "SI",
  cyprus: "CY",
  malta: "MT",
  iceland: "IS",
  luxembourg: "LU",
  georgia: "GE",
  armenia: "AM",
  azerbaijan: "AZ",
  uzbekistan: "UZ",
};

/**
 * Extracts a 2-letter ISO country code from country name, geo field, or emoji flag in text.
 */
export function extractCountryCode(
  geoCountry?: string | null,
  name?: string | null,
): string | null {
  if (geoCountry) {
    const trimmed = geoCountry.trim();
    if (trimmed.length === 2) {
      return trimmed.toUpperCase();
    }
    const mapped = COUNTRY_NAME_TO_CODE[trimmed.toLowerCase()];
    if (mapped) return mapped;
  }
  if (name) {
    for (let i = 0; i < name.length - 1; i++) {
      const c1 = name.codePointAt(i);
      if (c1 && c1 >= 0x1f1e6 && c1 <= 0x1f1ff) {
        const c2 = name.codePointAt(i + 2);
        if (c2 && c2 >= 0x1f1e6 && c2 <= 0x1f1ff) {
          const char1 = String.fromCharCode(65 + (c1 - 0x1f1e6));
          const char2 = String.fromCharCode(65 + (c2 - 0x1f1e6));
          return `${char1}${char2}`;
        }
      }
    }
    const lowerName = name.toLowerCase();
    for (const [cName, cCode] of Object.entries(COUNTRY_NAME_TO_CODE)) {
      if (lowerName.includes(cName)) {
        return cCode;
      }
    }
  }
  return null;
}

/**
 * Format relative time (e.g., "2 minutes ago", "1 hour ago")
 */
export function formatRelativeTime(timestamp: number): string {
  const now = Math.floor(Date.now() / 1000);
  const secondsAgo = now - timestamp;

  if (secondsAgo < 60) {
    return "just now";
  }

  const minutesAgo = Math.floor(secondsAgo / 60);
  if (minutesAgo < 60) {
    return `${minutesAgo} minute${minutesAgo !== 1 ? "s" : ""} ago`;
  }

  const hoursAgo = Math.floor(minutesAgo / 60);
  if (hoursAgo < 24) {
    return `${hoursAgo} hour${hoursAgo !== 1 ? "s" : ""} ago`;
  }

  const daysAgo = Math.floor(hoursAgo / 24);
  return `${daysAgo} day${daysAgo !== 1 ? "s" : ""} ago`;
}
