% Reference for 02-feedback-TF.json
%
%   step -> [sum + -] --e--> gain(2) --u--> P(s)=1/(s+1) --y--> scope
%              ^                                            |
%              +-------------------- y ---------------------+
%
% Continuous plant discretized with ZOH at Ts (matches the backend). P is
% strictly proper, so y[k] depends on u[k-1] -- the one-sample structure that
% makes the feedback loop well posed.
%
% Run in MATLAB (Control System Toolbox) to (re)generate the .ref.csv.

Ts = 0.1; end_time = 10;
t = (0:Ts:end_time)';
N = numel(t);

% ZOH-discretized 1/(s+1): y[k] = b1*u[k-1] + (-a2)*y[k-1].
[b, a] = tfdata(c2d(tf(1, [1 1]), Ts, 'zoh'), 'v');
b1 = b(2); a2 = a(2);

r = double(t >= 1.0);          % step-13, 0 -> 1 at t = 1

tf7  = zeros(N,1);             % transferFunction-7 output (y)
sum11 = zeros(N,1);            % sum-11 (e)
gain12 = zeros(N,1);           % gain-12 (u)
u_prev = 0.0; y_prev = 0.0;
for k = 1:N
    y = b1*u_prev - a2*y_prev; % TF output from past input/output
    e = r(k) - y;              % sum "+ -": reference minus feedback
    u = 2.0 * e;               % gain

    tf7(k) = y; sum11(k) = e; gain12(k) = u;
    u_prev = u; y_prev = y;    % this step's TF input becomes next step's past
end

header = {'t','transferFunction-7','scope-10','sum-11','gain-12','step-13'};
data = [t, tf7, tf7, sum11, gain12, r];

here = fileparts(mfilename('fullpath'));
if isempty(here); here = pwd; end
out = fullfile(here, '02-feedback-TF.ref.csv');
fid = fopen(out, 'w');
fprintf(fid, '%s\n', strjoin(header, ','));
fmt = [repmat('%.12g,', 1, size(data,2)-1) '%.12g\n'];
fprintf(fid, fmt, data.');
fclose(fid);
fprintf('wrote %s (%d samples)\n', out, size(data,1));
