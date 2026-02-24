-- Gemini Protocol Transformer (Lua)
--
-- A complete Lua implementation of the Gemini ↔ UIF protocol transformer.
-- Demonstrates all 4 transform hooks: request_out, request_in,
-- response_in, response_out.
--
-- Usage: Set this script as the `lua_script` on a Gemini-type provider.
-- The Lua hooks will take over protocol conversion instead of the
-- hardcoded GeminiTransformer.
--
-- Covers: text, images (inline + file), tool calls, tool results,
--         thinking blocks, system instructions, tool choice, parameters.
-- Does NOT cover: streaming (handled by hardcoded transformer).

-- =========================================================================
-- Helpers
-- =========================================================================

local _call_counter = 0
local function gen_call_id()
    _call_counter = _call_counter + 1
    return "call_lua_" .. tostring(_call_counter) .. "_" .. tostring(math.random(100000, 999999))
end

-- Gemini finish reason → UIF stop reason
local FINISH_TO_STOP = {
    STOP           = "end_turn",
    MAX_TOKENS     = "max_tokens",
    SAFETY         = "content_filter",
    RECITATION     = "content_filter",
    BLOCKLIST      = "content_filter",
    PROHIBITED_CONTENT = "content_filter",
    SPII           = "content_filter",
}

-- UIF stop reason → Gemini finish reason
local STOP_TO_FINISH = {
    end_turn       = "STOP",
    max_tokens     = "MAX_TOKENS",
    length         = "MAX_TOKENS",
    tool_use       = "STOP",
    stop_sequence  = "STOP",
    content_filter = "SAFETY",
}

-- UIF tool_choice.type → Gemini mode
local TOOL_CHOICE_MODE = {
    auto     = "AUTO",
    any      = "ANY",
    required = "ANY",
    none     = "NONE",
    tool     = "ANY",
}

-- Convert a single Gemini part → list of UIF content blocks
local function part_to_unified(part)
    local out = {}

    if part.text ~= nil then
        if part.thought then
            out[#out + 1] = { type = "thinking", text = part.text }
        else
            out[#out + 1] = { type = "text", text = part.text }
            if part.thoughtSignature then
                out[#out + 1] = { type = "thinking", text = "", signature = part.thoughtSignature }
            end
        end
        return out
    end

    if part.functionCall then
        local fc = part.functionCall
        out[#out + 1] = {
            type  = "tool_use",
            id    = gen_call_id(),
            name  = fc.name or "",
            input = fc.args or {},
        }
        if part.thoughtSignature then
            out[#out + 1] = { type = "thinking", text = "", signature = part.thoughtSignature }
        end
        return out
    end

    if part.functionResponse then
        local fr = part.functionResponse
        out[#out + 1] = {
            type        = "tool_result",
            tool_use_id = fr.name or "",
            content     = fr.response,
            is_error    = false,
        }
        return out
    end

    if part.inlineData then
        local d = part.inlineData
        out[#out + 1] = {
            type        = "image",
            source_type = "base64",
            media_type  = d.mimeType or "",
            data        = d.data or "",
        }
        if part.thoughtSignature then
            out[#out + 1] = { type = "thinking", text = "", signature = part.thoughtSignature }
        end
        return out
    end

    if part.fileData then
        local fd = part.fileData
        out[#out + 1] = {
            type        = "image",
            source_type = "url",
            media_type  = fd.mimeType or "",
            data        = fd.fileUri or "",
        }
        if part.thoughtSignature then
            out[#out + 1] = { type = "thinking", text = "", signature = part.thoughtSignature }
        end
        return out
    end

    return out
end

-- Convert a UIF content block → Gemini part (or nil for signature-only thinking)
local function unified_to_part(c)
    if c.type == "text" then
        return { text = c.text }
    end

    if c.type == "thinking" then
        if (not c.text or c.text == "") and c.signature then
            return nil  -- signature-only; handled separately
        end
        return { thought = true, text = c.text }
    end

    if c.type == "tool_use" then
        return { functionCall = { name = c.name, args = c.input or {} } }
    end

    if c.type == "tool_result" then
        return {
            functionResponse = {
                name     = c.tool_use_id or "",
                response = c.content,
            }
        }
    end

    if c.type == "image" then
        if c.source_type == "url" then
            return { fileData = { mimeType = c.media_type or "", fileUri = c.data or "" } }
        end
        return { inlineData = { mimeType = c.media_type or "", data = c.data or "" } }
    end

    return nil
end

-- Convert a list of UIF content blocks → Gemini parts
-- Re-attaches signature-only thinking blocks to the preceding part.
local function unified_contents_to_parts(contents)
    local parts = {}
    if not contents then return parts end
    for i = 1, #contents do
        local c = contents[i]
        if c.type == "thinking" and (not c.text or c.text == "") and c.signature then
            if #parts > 0 then
                parts[#parts].thoughtSignature = c.signature
            end
        else
            local p = unified_to_part(c)
            if p then
                parts[#parts + 1] = p
            end
        end
    end
    return parts
end

-- Find function name by tool_use_id in all messages
local function find_function_name(tool_use_id, messages)
    if not messages then return nil end
    for i = 1, #messages do
        local msg = messages[i]
        local content = msg.content
        if content then
            for j = 1, #content do
                local c = content[j]
                if c.type == "tool_use" and c.id == tool_use_id then
                    return c.name
                end
            end
        end
        local tcs = msg.tool_calls
        if tcs then
            for j = 1, #tcs do
                if tcs[j].id == tool_use_id then
                    return tcs[j].name
                end
            end
        end
    end
    return nil
end

-- =========================================================================
-- Hook 1: on_transform_request_out  (Gemini Client → UIF)
-- =========================================================================

function on_transform_request_out(ctx)
    local raw = ctx:get_request()
    if not raw then return end

    -- Only handle Gemini-format input (has "contents", not "messages")
    if raw.contents == nil then return end

    local messages = {}

    -- Convert contents → UIF messages
    local contents = raw.contents
    if contents then
        for i = 1, #contents do
            local c = contents[i]
            local role_str = c.role or "user"
            local role = (role_str == "model") and "assistant" or "user"

            local unified_content = {}
            local tool_calls = {}
            local tool_call_id = nil

            local parts = c.parts or {}
            for j = 1, #parts do
                local items = part_to_unified(parts[j])
                for k = 1, #items do
                    local item = items[k]
                    if item.type == "tool_use" then
                        tool_calls[#tool_calls + 1] = {
                            id = item.id,
                            name = item.name,
                            arguments = item.input,
                        }
                    end
                    if item.type == "tool_result" then
                        tool_call_id = item.tool_use_id
                    end
                    unified_content[#unified_content + 1] = item
                end
            end

            -- Detect tool role
            local effective_role = role
            if role == "user" and tool_call_id then
                local all_tool_result = true
                for j = 1, #unified_content do
                    if unified_content[j].type ~= "tool_result" then
                        all_tool_result = false
                        break
                    end
                end
                if all_tool_result then
                    effective_role = "tool"
                end
            end

            messages[#messages + 1] = {
                role         = effective_role,
                content      = unified_content,
                tool_calls   = (#tool_calls > 0) and tool_calls or nil,
                tool_call_id = tool_call_id,
            }
        end
    end

    -- System instruction
    local system = nil
    local si = raw.systemInstruction
    if si and si.parts then
        local texts = {}
        for i = 1, #si.parts do
            local t = si.parts[i].text
            if t then texts[#texts + 1] = t end
        end
        if #texts > 0 then
            system = table.concat(texts, "\n")
        end
    end

    -- Generation config → parameters
    local gen = raw.generationConfig or {}
    local parameters = {
        temperature    = gen.temperature,
        max_tokens     = gen.maxOutputTokens,
        top_p          = gen.topP,
        top_k          = gen.topK,
        stop_sequences = gen.stopSequences,
        stream         = false,
    }

    -- Tools
    local tools = nil
    if raw.tools then
        tools = {}
        for i = 1, #raw.tools do
            local group = raw.tools[i]
            local decls = group.functionDeclarations or {}
            for j = 1, #decls do
                local d = decls[j]
                tools[#tools + 1] = {
                    name         = d.name or "",
                    description  = d.description,
                    input_schema = d.parameters or {},
                    tool_type    = "function",
                }
            end
        end
    end

    -- Tool choice
    local tool_choice = nil
    if raw.toolConfig then
        local fcc = raw.toolConfig.functionCallingConfig or {}
        local mode = fcc.mode or ""
        if mode == "AUTO" then
            tool_choice = { type = "auto" }
        elseif mode == "ANY" then
            local names = fcc.allowedFunctionNames
            if names and #names == 1 then
                tool_choice = { type = "tool", name = names[1] }
            else
                tool_choice = { type = "any" }
            end
        elseif mode == "NONE" then
            tool_choice = { type = "none" }
        end
    end

    ctx:set_unified({
        model           = raw.model or ctx:get_model(),
        messages        = messages,
        system          = system,
        parameters      = parameters,
        tools           = tools,
        tool_choice     = tool_choice,
        client_protocol = "gemini",
    })
end

-- =========================================================================
-- Hook 2: on_transform_request_in  (UIF → Gemini Provider)
-- =========================================================================

function on_transform_request_in(ctx)
    local uif = ctx:get_unified()
    if not uif then return end

    -- Only produce Gemini-format output
    if ctx:get_provider_protocol() ~= "gemini" then return end

    -- Build contents with consecutive same-role merging
    local contents = {}
    local pending_role = nil
    local pending_parts = {}

    local all_messages = uif.messages or {}
    for i = 1, #all_messages do
        local msg = all_messages[i]
        local role = (msg.role == "assistant") and "model" or "user"

        -- Build parts for this message
        local msg_parts = {}

        -- Separate tool_result from other content
        local tool_results = {}
        local other_content = {}
        local content = msg.content or {}
        for j = 1, #content do
            local c = content[j]
            if c.type == "tool_result" then
                tool_results[#tool_results + 1] = c
            else
                other_content[#other_content + 1] = c
            end
        end

        -- Convert other content
        local parts_from_content = unified_contents_to_parts(other_content)
        for j = 1, #parts_from_content do
            msg_parts[#msg_parts + 1] = parts_from_content[j]
        end

        -- Convert tool results with name lookup
        for j = 1, #tool_results do
            local tr = tool_results[j]
            local fn_name = find_function_name(tr.tool_use_id, all_messages) or tr.tool_use_id
            msg_parts[#msg_parts + 1] = {
                functionResponse = {
                    name     = fn_name,
                    response = tr.content,
                }
            }
        end

        -- Append tool_calls not already in content
        local existing_tc_ids = {}
        for j = 1, #content do
            if content[j].type == "tool_use" then
                existing_tc_ids[content[j].id] = true
            end
        end
        local tcs = msg.tool_calls or {}
        for j = 1, #tcs do
            if not existing_tc_ids[tcs[j].id] then
                msg_parts[#msg_parts + 1] = {
                    functionCall = { name = tcs[j].name, args = tcs[j].arguments or {} }
                }
            end
        end

        -- Merge consecutive same-role messages
        if pending_role == role then
            for j = 1, #msg_parts do
                pending_parts[#pending_parts + 1] = msg_parts[j]
            end
        else
            if pending_role and #pending_parts > 0 then
                contents[#contents + 1] = { role = pending_role, parts = pending_parts }
            end
            pending_role = role
            pending_parts = {}
            for j = 1, #msg_parts do
                pending_parts[#pending_parts + 1] = msg_parts[j]
            end
        end
    end
    if pending_role and #pending_parts > 0 then
        contents[#contents + 1] = { role = pending_role, parts = pending_parts }
    end

    local request = { contents = contents }

    -- System instruction
    if uif.system and uif.system ~= "" then
        request.systemInstruction = { parts = { { text = uif.system } } }
    end

    -- Generation config
    local gen = {}
    local p = uif.parameters or {}
    if p.temperature    ~= nil then gen.temperature     = p.temperature    end
    if p.max_tokens     ~= nil then gen.maxOutputTokens = p.max_tokens     end
    if p.top_p          ~= nil then gen.topP            = p.top_p          end
    if p.top_k          ~= nil then gen.topK            = p.top_k          end
    if p.stop_sequences ~= nil then gen.stopSequences   = p.stop_sequences end
    if next(gen) then
        request.generationConfig = gen
    end

    -- Tools
    local tools = uif.tools
    if tools and #tools > 0 then
        local decls = {}
        for i = 1, #tools do
            local t = tools[i]
            local decl = {
                name       = t.name,
                parameters = t.input_schema or {},
            }
            if t.description then decl.description = t.description end
            decls[#decls + 1] = decl
        end
        request.tools = { { functionDeclarations = decls } }
    end

    -- Tool choice
    if uif.tool_choice then
        local tc_type = uif.tool_choice.type or "auto"
        local mode = TOOL_CHOICE_MODE[tc_type] or "AUTO"
        local fcc = { mode = mode }
        if tc_type == "tool" and uif.tool_choice.name then
            fcc.allowedFunctionNames = { uif.tool_choice.name }
        end
        request.toolConfig = { functionCallingConfig = fcc }
    end

    ctx:set_request(request)
end

-- =========================================================================
-- Hook 3: on_transform_response_in  (Gemini Provider → UIF)
-- =========================================================================

function on_transform_response_in(ctx)
    local raw = ctx:get_response()
    if not raw then return end

    -- Only handle Gemini-format responses (has "candidates")
    if raw.candidates == nil then return end

    local candidates = raw.candidates or {}
    local candidate = candidates[1] or {}

    local parts = (candidate.content or {}).parts or {}

    local content = {}
    local tool_calls = {}

    for i = 1, #parts do
        local items = part_to_unified(parts[i])
        for j = 1, #items do
            local item = items[j]
            if item.type == "tool_use" then
                tool_calls[#tool_calls + 1] = {
                    id        = item.id,
                    name      = item.name,
                    arguments = item.input,
                }
            end
            content[#content + 1] = item
        end
    end

    -- Stop reason
    local finish = candidate.finishReason
    local stop_reason = FINISH_TO_STOP[finish or ""] or "end_turn"

    -- Usage
    local um = raw.usageMetadata or {}
    local usage = {
        input_tokens  = um.promptTokenCount or 0,
        output_tokens = um.candidatesTokenCount or 0,
    }
    if um.cachedContentTokenCount then
        usage.cache_read_tokens = um.cachedContentTokenCount
    end

    local resp_id = raw.responseId or ""

    ctx:set_unified({
        id          = resp_id,
        model       = ctx:get_model(),
        content     = content,
        stop_reason = (#content > 0 or finish) and stop_reason or nil,
        usage       = usage,
        tool_calls  = (#tool_calls > 0) and tool_calls or nil,
    })
end

-- =========================================================================
-- Hook 4: on_transform_response_out  (UIF → Gemini Client)
-- =========================================================================

function on_transform_response_out(ctx)
    local uif = ctx:get_unified()
    if not uif then return end

    -- Only produce Gemini-format output
    if ctx:get_client_protocol() ~= "gemini" then return end

    local parts = unified_contents_to_parts(uif.content or {})

    -- Append tool_calls not already in content
    local tcs = uif.tool_calls or {}
    for i = 1, #tcs do
        parts[#parts + 1] = {
            functionCall = { name = tcs[i].name, args = tcs[i].arguments or {} }
        }
    end

    local finish_reason = STOP_TO_FINISH[uif.stop_reason or ""] or "STOP"

    local usage = uif.usage or {}
    local input_tokens  = usage.input_tokens or 0
    local output_tokens = usage.output_tokens or 0

    ctx:set_response({
        candidates = {
            {
                content      = { role = "model", parts = parts },
                finishReason = finish_reason,
            }
        },
        usageMetadata = {
            promptTokenCount     = input_tokens,
            candidatesTokenCount = output_tokens,
            totalTokenCount      = input_tokens + output_tokens,
        },
        modelVersion = uif.model or "",
    })
end
