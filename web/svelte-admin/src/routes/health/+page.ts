// Disable SSR for the health check page
// The health check page relies on client-side authentication state
// and makes API calls that require the auth token from localStorage
export const ssr = false;
