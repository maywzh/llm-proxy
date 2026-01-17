// Disable SSR for the credentials page
// The credentials page relies on client-side authentication state
// and makes API calls that require the auth token from localStorage
export const ssr = false;
