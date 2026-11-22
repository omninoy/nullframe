import { NullFrameClient } from "../src/index.mts";
import { FakeNullFrame } from "./fake-nullframe.mts";

export const TOKEN = "test-token-abc123";

/** Start a fake app, point a client at it, and always shut the server down. */
export async function withClient<T>(
  work: (client: NullFrameClient, fake: FakeNullFrame) => Promise<T>,
): Promise<T> {
  const fake = await new FakeNullFrame().start();
  try {
    const client = new NullFrameClient({
      token: TOKEN,
      port: fake.port,
      timeoutMs: 5_000,
      env: {},
    });
    return await work(client, fake);
  } finally {
    await fake.stop();
  }
}
