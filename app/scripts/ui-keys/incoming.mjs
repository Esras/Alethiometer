// Incoming: what dismissing leaves behind.

const topicWithOne = (t) =>
  t.page.locator('button.topic:not(.unsorted)', { has: t.page.locator('.count', { hasText: /^1$/ }) });

export const checks = {
  'incoming: a topic emptied by dismissing leaves the list once you move to another topic': async (t) => {
    const one = topicWithOne(t).first();
    await t.ok((await one.count()) === 1, 'the sample data has no topic with one signal');
    const key = await one.getAttribute('data-topic-key');
    await one.click();
    await t.steps('d');
    await t.visible(t.page.locator(`[data-topic-key="${key}"]`), 'the dismissed row should stay until you move off');
    await t.steps(']');
    await t.gone(t.page.locator(`[data-topic-key="${key}"]`), 'the emptied topic is still listed after moving off');
  },
};
