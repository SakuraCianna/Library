import re

path = 'src/__tests__/App.test.tsx'
with open(path, 'r', encoding='utf-8') as f:
    content = f.read()

# Replace any occurrence of ChatMessage mock
# We can just look for role: "user" | "assistant" and add properties before it.
content = re.sub(
    r'(role:\s*"(user|assistant|system)",)',
    r'conversationId: "test", \1',
    content
)

content = re.sub(
    r'(sources:\s*\[[^\]]*\],?)',
    r'\1 createdAt: new Date().toISOString(),',
    content
)

with open(path, 'w', encoding='utf-8') as f:
    f.write(content)
