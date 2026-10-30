import sys,json,time
mode=sys.argv[1]
def send(value): print(json.dumps(value),flush=True)
if mode=='oversize':
 sys.stdout.write('x'*(1024*1024+2));sys.stdout.flush();time.sleep(5);sys.exit()
if mode in ('timeout','no_read'):
 send({'ready':True});time.sleep(5);sys.exit()
if mode=='invalid':
 print('secret_test_marker',flush=True);sys.exit()
if mode=='claude_status':
 send({'loggedIn':True,'authMethod':'api_key','email':'do-not-display@example.test'});sys.exit()
for line in sys.stdin:
 request=json.loads(line);method=request.get('method');rid=request.get('id')
 if request.get('type')=='control_request':
  if request['request']['subtype']=='mcp_status':
   send({'type':'control_response','response':{'subtype':'success','request_id':request['request_id'],'response':{'mcpServers':[{'name':'hantong','status':'connected'}] if mode!='claude_mcp_missing' else []}}});continue
  assert request['request']['subtype']=='initialize'
  send({'type':'control_response','response':{'subtype':'success','request_id':request['request_id'],'response':{'models':[{'value':'fake-claude','displayName':'Fake Claude','supportsEffort':True,'supportedEffortLevels':['low','high'],'supportsFastMode':True}],'fast_mode_state':'on'}}});continue
 if mode=='mcp':
  if method=='initialize':send({'id':rid,'result':{'protocolVersion':'2025-11-25'}})
  elif method=='tools/list':send({'id':rid,'result':{'tools':[{'name':'mail.accounts.list','description':'Synthetic authorized account query','inputSchema':{'type':'object'}}]}})
  elif method=='tools/call':
   assert request['params']['name']=='mail.accounts.list';send({'id':rid,'result':{'content':[{'type':'text','text':'synthetic account'}],'isError':False}})
  continue
 if method=='initialize':send({'id':rid,'result':{}})
 elif method=='account/read':send({'id':rid,'result':{'account':{'type':'chatgpt','email':'do-not-display@example.test'},'requiresOpenaiAuth':True}})
 elif method=='account/login/start':
  send({'method':'account/login/completed','params':{'loginId':'fake-login','success':True}})
  send({'id':rid,'result':{'type':'chatgpt','loginId':'fake-login','authUrl':'https://chatgpt.com/fake?state=synthetic'}})
 elif method=='model/list':send({'id':rid,'result':{'data':[{'model':'fake-model','displayName':'Fake model','supportedReasoningEfforts':[{'reasoningEffort':'low'},{'reasoningEffort':'high'},{'reasoningEffort':'xhigh'}],'serviceTiers':[{'id':'priority'}],'isDefault':True},{'model':'limited-model','displayName':'Limited','supportedReasoningEfforts':[],'serviceTiers':[]}],'nextCursor':None}})
 elif method=='thread/start':send({'id':rid,'result':{'thread':{'id':'T'}}})
 elif method=='turn/start':
  assert request['params']['approvalPolicy']=='onRequest'
  if mode=='settings':
   assert request['params']['model']=='fake-model' and request['params']['effort']=='high' and request['params']['serviceTier']=='priority'
  if mode=='agent':
   send({'id':78,'method':'item/tool/call','params':{'threadId':'T','turnId':'Q','namespace':'hantong','tool':'mail_accounts_list','arguments':{}}})
  assert request['params']['sandboxPolicy']['access']['type']=='restricted'
  send({'id':77,'method':'item/commandExecution/requestApproval','params':{'command':'not-executed'}})
  denied=json.loads(sys.stdin.readline());assert denied['id']==77 and 'error' in denied
  send({'method':'item/agentMessage/delta','params':{'threadId':'T','turnId':'Q','delta':'hello '}})
  send({'id':rid,'result':{'turn':{'id':'Q'}}})
  if mode=='agent':
   response=json.loads(sys.stdin.readline());assert response['id']==78 and response['result']['success']==True
  send({'method':'item/agentMessage/delta','params':{'threadId':'WRONG','turnId':'Q','delta':'must-not-show'}})
  send({'method':'item/agentMessage/delta','params':{'threadId':'T','turnId':'Q','delta':'world'}})
  send({'method':'turn/completed','params':{'threadId':'T','turn':{'id':'Q','status':'completed'}}})
