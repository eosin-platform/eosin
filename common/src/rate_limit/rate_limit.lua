local key            = KEYS[1]
local burst_limit    = tonumber(ARGV[1])
local burst_window   = tonumber(ARGV[2])
local long_limit     = tonumber(ARGV[3])
local long_window    = tonumber(ARGV[4])
local now_ms         = tonumber(ARGV[5])
local max_list_size  = tonumber(ARGV[6])

-- Insert current timestamp at head (newest first)
redis.call("LPUSH", key, now_ms)
-- Bound list length to control CPU usage
redis.call("LTRIM", key, 0, max_list_size - 1)

local burst_threshold = now_ms - burst_window
local long_threshold  = now_ms - long_window

local burst_count = 0
local long_count  = 0

-- Newest first
local entries = redis.call("LRANGE", key, 0, -1)
for i = 1, #entries do
    local ts = tonumber(entries[i])

    -- Only consider entries within the long window
    if ts >= long_threshold then
        long_count = long_count + 1
        -- And subset that are within the burst window
        if ts >= burst_threshold then
            burst_count = burst_count + 1
        end
    else
        -- List is newest→oldest, so once we hit an old entry we can stop
        break
    end
end

if burst_count > burst_limit or long_count > long_limit then
    return 0
end

return 1