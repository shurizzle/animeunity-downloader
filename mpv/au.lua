mp.add_hook("on_load", 50, function()
	local url = mp.get_property("stream-open-filename")
	if type(url) ~= "string" then
		return
	end

	local res, _ = mp.command_native({
		name = "subprocess",
		args = { "aumpv", url },
		playback_only = false,
		capture_stdout = true,
		capture_stderr = true,
	})
	if not res or res.error or not res.stdout then
		return
	end
	local info = require("mp.utils").parse_json(res.stdout)
	if not info then
		return
	end

	if info.type == "video" then
		mp.set_property("stream-open-filename", info.url)
	elseif info.type == "playlist" then
		if info.items and #info.items > 0 then
			for i = #info.items, 1, -1 do
				mp.commandv("loadfile", info.items[i], "insert-next")
			end
		end
		mp.commandv("playlist-remove", "current")
	end
end)

