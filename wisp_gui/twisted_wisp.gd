extends Control

const FlowGraphView = preload("res://flow_graph_view.tscn")

var wisp: TwistedWisp = null

func _ready():
	var config = ConfigFile.new()
	config.load("res://wisp.ini")
	var wisp_exe_path = config.get_value("wisp", "executable_path")
	var wisp_core_path = config.get_value("wisp", "core_path")
	wisp = TwistedWisp.create(wisp_exe_path, wisp_core_path)
	var graph = FlowGraphView.instantiate()
	graph.wisp = wisp
	add_child(graph)
	graph.grab_focus()


func _process(_delta):
	pass
