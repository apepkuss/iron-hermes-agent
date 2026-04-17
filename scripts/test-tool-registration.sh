#!/bin/bash
# 手动验证 skill/memory 工具注册是否生效。
# 使用方法:
#   1. 启动 iron-server (确保 LLM 后端可用)
#   2. 运行本脚本: bash scripts/test-tool-registration.sh

set -euo pipefail

SERVER_URL="${SERVER_URL:-http://localhost:9069}"
MODEL="${LLM_MODEL:-iron-hermes}"

GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

pass() { echo -e "${GREEN}PASS${NC}: $1"; }
fail() { echo -e "${RED}FAIL${NC}: $1"; exit 1; }

echo "=== Tool Registration Verification ==="
echo "Server: $SERVER_URL"
echo ""

# 1. Health check
echo "--- Health Check ---"
HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" "$SERVER_URL/health")
[ "$HTTP_CODE" = "200" ] && pass "Server is healthy" || fail "Server returned $HTTP_CODE"

# 2. Test: List skills
echo ""
echo "--- Test: List Skills ---"
RESPONSE=$(curl -s "$SERVER_URL/v1/chat/completions" \
  -H "Content-Type: application/json" \
  -d "{
    \"model\": \"$MODEL\",
    \"messages\": [{\"role\": \"user\", \"content\": \"使用 skills_list 工具，列出所有可用的 skills，只返回数量即可。\"}],
    \"stream\": false
  }")

echo "Response (first 500 chars):"
echo "$RESPONSE" | python3 -c "
import sys, json
try:
    d = json.load(sys.stdin)
    content = d['choices'][0]['message']['content']
    print(content[:500])
except Exception as e:
    print(f'Parse error: {e}')
"

# 3. Test: View a skill
echo ""
echo "--- Test: View Skill ---"
RESPONSE=$(curl -s "$SERVER_URL/v1/chat/completions" \
  -H "Content-Type: application/json" \
  -d "{
    \"model\": \"$MODEL\",
    \"messages\": [{\"role\": \"user\", \"content\": \"使用 skill_view 工具查看名为 arxiv 的 skill 的完整内容，然后告诉我这个 skill 的描述是什么。\"}],
    \"stream\": false
  }")

echo "Response (first 500 chars):"
echo "$RESPONSE" | python3 -c "
import sys, json
try:
    d = json.load(sys.stdin)
    content = d['choices'][0]['message']['content']
    print(content[:500])
except Exception as e:
    print(f'Parse error: {e}')
"

# 4. Test: Memory tool
echo ""
echo "--- Test: Memory Tool ---"
RESPONSE=$(curl -s "$SERVER_URL/v1/chat/completions" \
  -H "Content-Type: application/json" \
  -d "{
    \"model\": \"$MODEL\",
    \"messages\": [{\"role\": \"user\", \"content\": \"使用 memory 工具，向 memory 目标中添加一条记录: 'integration test entry'。然后确认添加成功。\"}],
    \"stream\": false
  }")

echo "Response (first 500 chars):"
echo "$RESPONSE" | python3 -c "
import sys, json
try:
    d = json.load(sys.stdin)
    content = d['choices'][0]['message']['content']
    print(content[:500])
except Exception as e:
    print(f'Parse error: {e}')
"

# 5. Cleanup
echo ""
echo "--- Cleanup ---"
curl -s "$SERVER_URL/v1/chat/completions" \
  -H "Content-Type: application/json" \
  -d "{
    \"model\": \"$MODEL\",
    \"messages\": [{\"role\": \"user\", \"content\": \"使用 memory 工具，从 memory 目标中删除包含 'integration test' 的记录。\"}],
    \"stream\": false
  }" > /dev/null

pass "Cleanup done"

# 6. Test: execute_code (Python)
echo ""
echo "--- Test: Execute Code (Python) ---"
RESPONSE=$(curl -s --max-time 600 "$SERVER_URL/v1/chat/completions" \
  -H "Content-Type: application/json" \
  -d "{
    \"model\": \"$MODEL\",
    \"messages\": [{\"role\": \"user\", \"content\": \"使用 execute_code 工具，执行 Python 代码: print(2+3)，然后告诉我结果\"}],
    \"stream\": false
  }")

echo "Response (first 500 chars):"
echo "$RESPONSE" | python3 -c "
import sys, json
try:
    d = json.load(sys.stdin)
    content = d['choices'][0]['message']['content']
    print(content[:500])
except Exception as e:
    print(f'Parse error: {e}')
"

echo ""
echo "=== Verification Complete ==="
echo "请人工检查以上输出，确认 LLM 正确调用了 skills_list、skill_view、memory、execute_code 工具。"
