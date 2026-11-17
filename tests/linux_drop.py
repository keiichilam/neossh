"""Deliver real XDND URI-list events to one owned native window."""
import ctypes as c
import time


def drop_files(x11, display, target, paths):
    class Client(c.Structure):
        _fields_ = [('type', c.c_int), ('serial', c.c_ulong), ('send_event', c.c_int),
                    ('display', c.c_void_p), ('window', c.c_ulong), ('message_type', c.c_ulong),
                    ('format', c.c_int), ('data', c.c_long * 5)]
    class Selection(c.Structure):
        _fields_ = [('type', c.c_int), ('serial', c.c_ulong), ('send_event', c.c_int),
                    ('display', c.c_void_p), ('requestor', c.c_ulong), ('selection', c.c_ulong),
                    ('target', c.c_ulong), ('property', c.c_ulong), ('time', c.c_ulong)]
    class Request(c.Structure):
        _fields_ = [('type', c.c_int), ('serial', c.c_ulong), ('send_event', c.c_int),
                    ('display', c.c_void_p), ('owner', c.c_ulong), ('requestor', c.c_ulong),
                    ('selection', c.c_ulong), ('target', c.c_ulong), ('property', c.c_ulong), ('time', c.c_ulong)]
    class Event(c.Union):
        _fields_ = [('client', Client), ('selection', Selection), ('request', Request), ('padding', c.c_long * 24)]
    x11.XInternAtom.argtypes = [c.c_void_p, c.c_char_p, c.c_int]
    x11.XInternAtom.restype = c.c_ulong
    atom = lambda name: x11.XInternAtom(display, name.encode(), 0)
    x11.XDefaultRootWindow.argtypes = [c.c_void_p]
    x11.XDefaultRootWindow.restype = c.c_ulong
    x11.XCreateSimpleWindow.argtypes = [c.c_void_p, c.c_ulong, c.c_int, c.c_int, c.c_uint, c.c_uint, c.c_uint, c.c_ulong, c.c_ulong]
    x11.XCreateSimpleWindow.restype = c.c_ulong
    x11.XDestroyWindow.argtypes = [c.c_void_p, c.c_ulong]
    x11.XSetSelectionOwner.argtypes = [c.c_void_p, c.c_ulong, c.c_ulong, c.c_ulong]
    x11.XSendEvent.argtypes = [c.c_void_p, c.c_ulong, c.c_int, c.c_long, c.POINTER(Event)]
    x11.XCheckTypedEvent.argtypes = [c.c_void_p, c.c_int, c.POINTER(Event)]
    x11.XChangeProperty.argtypes = [c.c_void_p, c.c_ulong, c.c_ulong, c.c_ulong, c.c_int, c.c_int, c.c_char_p, c.c_int]
    source = x11.XCreateSimpleWindow(display, x11.XDefaultRootWindow(display), 0, 0, 1, 1, 0, 0, 0)
    selection, uri = atom('XdndSelection'), atom('text/uri-list')
    def send(name, values):
        event = Event()
        event.client.type, event.client.display, event.client.window = 33, display, target
        event.client.message_type, event.client.format = atom(name), 32
        for index, value in enumerate(values):
            event.client.data[index] = value
        assert x11.XSendEvent(display, target, 0, 0, c.byref(event))
        x11.XSync(display, 0)
    try:
        x11.XSetSelectionOwner(display, selection, source, 0)
        send('XdndEnter', [source, 5 << 24, uri, 0, 0])
        send('XdndPosition', [source, 0, (500 << 16) | 400, 0, atom('XdndActionCopy')])
        event = Event()
        deadline = time.monotonic() + 5
        while not x11.XCheckTypedEvent(display, 30, c.byref(event)):
            assert time.monotonic() < deadline, 'XDND selection request missing'
            time.sleep(.01)
        request = event.request
        assert request.owner == source and request.requestor == target and request.target == uri
        payload = ('\r\n'.join(path.as_uri() for path in paths) + '\r\n').encode()
        prop = request.property or request.target
        x11.XChangeProperty(display, target, prop, uri, 8, 0, payload, len(payload))
        reply = Event()
        reply.selection = Selection(31, 0, 1, display, target, selection, uri, prop, request.time)
        assert x11.XSendEvent(display, target, 0, 0, c.byref(reply))
        x11.XSync(display, 0)
        time.sleep(.3)
        send('XdndDrop', [source, 0, 0, 0, 0])
        deadline = time.monotonic() + 5
        while True:
            if x11.XCheckTypedEvent(display, 33, c.byref(event)):
                if event.client.message_type == atom('XdndFinished'):
                    assert event.client.data[1] & 1, 'XDND drop rejected'
                    break
            assert time.monotonic() < deadline, 'XDND completion missing'
            time.sleep(.01)
    finally:
        x11.XDestroyWindow(display, source)
        x11.XSync(display, 0)
