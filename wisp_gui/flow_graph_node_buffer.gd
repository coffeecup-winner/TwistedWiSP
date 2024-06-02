extends FlowGraphNode


func _on_open_file_selected(f):
	var buffer_name = flow_node.flow().load_wave_file(f)
	flow_node.set_data_buffer(buffer_name)


func _on_btn_open_pressed():
	var fd = FileDialog.new()
	fd.access = FileDialog.ACCESS_FILESYSTEM
	fd.dialog_hide_on_ok = true
	fd.file_mode = FileDialog.FILE_MODE_OPEN_FILE
	fd.filters = PackedStringArray(["*.wav ; Waveform Audio Files"])
	fd.title = "Open an audio file"
	fd.use_native_dialog = true
	fd.connect("file_selected", _on_open_file_selected)
	fd.popup()
